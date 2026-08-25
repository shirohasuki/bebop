#include "verilator.h"
#include "VBBSimHarness.h"
#include "verilated.h"
#include "verilated_fst_c.h"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <deque>
#include <mutex>
#include <optional>
#include <unordered_map>
#include <vector>

// Context management
extern "C" void *verilator_context_new() { return new VerilatedContext; }

extern "C" void verilator_context_free(void *ctx) {
  delete static_cast<VerilatedContext *>(ctx);
}

extern "C" void verilator_context_time_inc(void *ctx, uint64_t add) {
  static_cast<VerilatedContext *>(ctx)->timeInc(add);
}

extern "C" uint64_t verilator_context_time(void *ctx) {
  return static_cast<VerilatedContext *>(ctx)->time();
}

extern "C" void verilator_context_command_args(void *ctx, int argc,
                                               const char **argv) {
  static_cast<VerilatedContext *>(ctx)->commandArgs(argc,
                                                    const_cast<char **>(argv));
}

extern "C" void verilator_context_trace_ever_on(void *ctx, bool on) {
  static_cast<VerilatedContext *>(ctx)->traceEverOn(on);
}

// Top module
extern "C" void *verilator_top_new(void *ctx) {
  return new VBBSimHarness{static_cast<VerilatedContext *>(ctx)};
}

extern "C" void verilator_top_free(void *top) {
  delete static_cast<VBBSimHarness *>(top);
}

extern "C" void verilator_top_eval(void *top) {
  static_cast<VBBSimHarness *>(top)->eval();
}

extern "C" void verilator_top_trace(void *top, void *tfp, int levels) {
  static_cast<VBBSimHarness *>(top)->trace(static_cast<VerilatedFstC *>(tfp),
                                           levels);
}

// Top module signals
extern "C" void verilator_top_set_clock(void *top, uint8_t val) {
  static_cast<VBBSimHarness *>(top)->clock = val;
}

extern "C" void verilator_top_set_reset(void *top, uint8_t val) {
  static_cast<VBBSimHarness *>(top)->reset = val;
}

// =============================================================================
// SCU shared state (used by both DPI-C callbacks and MMIO tick polling)
// =============================================================================
#define SIM_EXIT_ADDR 0x60000000ULL
#define UART_TX_ADDR 0x60020000ULL

static std::vector<uint32_t> g_uart_tx;
static std::unordered_map<uint32_t, std::deque<uint8_t>> g_uart_rx;
static int32_t g_exit_code = 0;
static bool g_has_exit = false;
static std::mutex g_scu_mutex;

// =============================================================================
// rushB command state
//
// Commands are supplied by the Rust C ABI and consumed by one DPI bridge per
// accelerator. The Verilator model is single-threaded; calls into this state
// occur only while that model is being stepped.
// =============================================================================
struct RushBCommand {
  uint64_t xs1;
  uint64_t xs2;
  uint32_t funct7;
};

struct RushBChannel {
  std::optional<RushBCommand> pending;
  uint64_t probes = 0;
  uint64_t accepted = 0;
  uint64_t completed = 0;
  bool last_ready = false;
  bool last_retired = false;
  uint64_t inflight = 0;
};

static std::unordered_map<uint32_t, RushBChannel> g_rushb_channels;

extern "C" void verilator_rushb_clear() { g_rushb_channels.clear(); }

extern "C" void verilator_rushb_submit(uint32_t accelerator_id,
                                           uint64_t xs1, uint64_t xs2,
                                           uint32_t funct7) {
  auto &channel = g_rushb_channels[accelerator_id];
  if (channel.pending.has_value()) {
    fprintf(stderr,
            "verilator rushB only permits one pending command per "
            "accelerator; accepted commands may remain in flight\n");
    abort();
  }
  channel.pending = RushBCommand{xs1, xs2, funct7};
}

extern "C" void verilator_rushb_peek(uint32_t accelerator_id,
                                         uint8_t *valid, uint64_t *xs1,
                                         uint64_t *xs2, uint32_t *funct7) {
  if (valid == nullptr || xs1 == nullptr || xs2 == nullptr ||
      funct7 == nullptr) {
    fprintf(stderr, "verilator_rushb_peek received null output pointer\n");
    abort();
  }
  auto &channel = g_rushb_channels[accelerator_id];
  channel.probes++;
  if (!channel.pending.has_value()) {
    *valid = 0;
    *xs1 = 0;
    *xs2 = 0;
    *funct7 = 0;
    return;
  }
  const auto &cmd = *channel.pending;
  *valid = 1;
  *xs1 = cmd.xs1;
  *xs2 = cmd.xs2;
  *funct7 = cmd.funct7;
}

extern "C" void verilator_rushb_observe(uint32_t accelerator_id,
                                            uint8_t valid, uint8_t ready) {
  auto &channel = g_rushb_channels[accelerator_id];
  (void)valid;
  channel.last_ready = ready != 0;
}

extern "C" void verilator_rushb_accept(uint32_t accelerator_id) {
  auto &channel = g_rushb_channels[accelerator_id];
  if (!channel.pending.has_value()) {
    fprintf(stderr, "verilator rushB accepted an invalid command\n");
    abort();
  }
  channel.pending.reset();
  channel.accepted++;
  channel.inflight++;
}

extern "C" void
verilator_rushb_complete_on_accept(uint32_t accelerator_id) {
  auto &channel = g_rushb_channels[accelerator_id];
  if (channel.inflight == 0) {
    fprintf(stderr,
            "verilator rushB completed an invalid accepted command\n");
    abort();
  }
  channel.inflight--;
  channel.completed++;
}

extern "C" void verilator_rushb_report(uint32_t accelerator_id,
                                           uint8_t retired) {
  auto &channel = g_rushb_channels[accelerator_id];
  channel.last_retired = retired != 0;
  if (channel.inflight == 0) {
    return;
  }

  // The accelerator reports completions in GlobalROB retirement order, so
  // each pulse completes the oldest in-flight host command.
  if (retired) {
    channel.inflight--;
    channel.completed++;
  }
}

extern "C" uint64_t verilator_rushb_accepted(uint32_t accelerator_id) {
  return g_rushb_channels[accelerator_id].accepted;
}

extern "C" uint64_t verilator_rushb_completed(uint32_t accelerator_id) {
  return g_rushb_channels[accelerator_id].completed;
}

extern "C" uint64_t verilator_rushb_inflight(uint32_t accelerator_id) {
  return g_rushb_channels[accelerator_id].inflight;
}

extern "C" uint64_t verilator_rushb_probes(uint32_t accelerator_id) {
  return g_rushb_channels[accelerator_id].probes;
}

extern "C" bool verilator_rushb_last_ready(uint32_t accelerator_id) {
  return g_rushb_channels[accelerator_id].last_ready;
}

extern "C" bool verilator_rushb_last_retired(uint32_t accelerator_id) {
  return g_rushb_channels[accelerator_id].last_retired;
}

// =============================================================================
// SCU DPI-C interface
// Called from RTL via DPI-C when software writes to SCU registers
// (0x6000_0000 for sim_exit, 0x6002_0000 for UART).
// State is queryable from Rust via verilator_scu_*() helpers below.
//
// Note: Verilator uses DPI-C SCU (per-tile UART/exit), not AXI4 MMIO.
// The SCU is intercepted inside each BBTile and never reaches the system bus.
// =============================================================================

extern "C" void scu_uart_write(uint32_t hart_id, uint32_t ch) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  g_uart_tx.push_back(((hart_id & 0x00ffffffu) << 8) | (ch & 0xffu));
}

extern "C" int scu_uart_rx_valid(uint32_t hart_id) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  auto it = g_uart_rx.find(hart_id);
  return it != g_uart_rx.end() && !it->second.empty();
}

extern "C" void scu_uart_rx_sample(uint32_t hart_id, uint32_t pop,
                                   uint32_t *valid, uint32_t *data) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  if (valid == nullptr || data == nullptr) {
    fprintf(stderr, "scu_uart_rx_sample received null output pointer\n");
    abort();
  }

  auto it = g_uart_rx.find(hart_id);
  if (it == g_uart_rx.end() || it->second.empty()) {
    *valid = 0;
    *data = 0;
    return;
  }

  *valid = 1;
  *data = it->second.front();
  if (pop) {
    it->second.pop_front();
  }
}

extern "C" int scu_uart_peek(uint32_t hart_id) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  auto it = g_uart_rx.find(hart_id);
  if (it == g_uart_rx.end() || it->second.empty()) {
    return 0;
  }
  return it->second.front();
}

extern "C" int scu_uart_pop(uint32_t hart_id) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  auto it = g_uart_rx.find(hart_id);
  if (it == g_uart_rx.end() || it->second.empty()) {
    return 0;
  }
  uint8_t byte = it->second.front();
  it->second.pop_front();
  return byte;
}

extern "C" void scu_sim_exit(uint32_t hart_id, uint32_t code) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  g_exit_code = code;
  g_has_exit = true;
  printf("\n[SCU] sim_exit called: hart_id=%u, exit_code=%u\n", hart_id, code);
  fflush(stdout);
}

extern "C" bool verilator_scu_has_exit() {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  return g_has_exit;
}

extern "C" int32_t verilator_scu_exit_code() {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  return g_exit_code;
}

extern "C" void verilator_scu_push_uart_rx(uint32_t hart_id, uint32_t byte) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  g_uart_rx[hart_id].push_back((uint8_t)(byte & 0xff));
}

extern "C" uint32_t verilator_scu_drain_uart_tx(uint32_t *buf, uint32_t len) {
  std::lock_guard<std::mutex> lock(g_scu_mutex);
  uint32_t n = 0;
  while (n < len && n < g_uart_tx.size()) {
    buf[n] = g_uart_tx[n];
    n++;
  }
  g_uart_tx.erase(g_uart_tx.begin(), g_uart_tx.begin() + n);
  return n;
}

extern "C" void *verilator_trace_new() { return new VerilatedFstC; }

extern "C" void verilator_trace_free(void *tfp) {
  delete static_cast<VerilatedFstC *>(tfp);
}

extern "C" bool verilator_trace_open(void *tfp, const char *filename) {
  static_cast<VerilatedFstC *>(tfp)->open(filename);
  return true;
}

extern "C" void verilator_trace_dump(void *tfp, uint64_t timeui) {
  static_cast<VerilatedFstC *>(tfp)->dump(timeui);
}

extern "C" void verilator_trace_close(void *tfp) {
  static_cast<VerilatedFstC *>(tfp)->close();
}
