//===----- spike.cc -------- new spike mainloop maintained by bemu --------------------===//
//
// Copyright 2026 The Aerospace Corporation
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
//===----------------------------------------------------------------------===//
//
// Maintains the main stages of Spike execution:
//  1. Initialize Stage 1: Initialize a new Spike instance
//  2. Load the elf file into the memory. (This is done by bemu, not Spike.)
//  3. Initialize Stage 2: Initialize hart state after Rust has loaded the workload into BEMU memory.
//  4. Execute step by step.
//  5. Finish the execution.
//
//===----------------------------------------------------------------------===//

#include "btif.h"
#include "processor.h"
#include "mmu.h"
#include "extension.h"
#include "trap.h"
#include <cstring>
#include <cstdio>
#include <cstdlib>
#include <csignal>
#include <chrono>
#include <execinfo.h>
#include <unistd.h>
#include <mutex>

extern "C" {
    uint64_t handle_syscall_ffi(void*, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);
    bool should_exit_ffi(void*);
    int get_exit_code_ffi(void*);
}

static thread_local void* current_emu_state = nullptr;

extern "C" void* current_bemu_state() {
    return current_emu_state;
}

static void crash_handler(int sig) {
    void* bt[64];
    int n = backtrace(bt, 64);
    fprintf(stderr, "[FATAL] signal=%d in spike_wrapper\n", sig);
    backtrace_symbols_fd(bt, n, STDERR_FILENO);
    fflush(stderr);
    _Exit(128 + sig);
}

void init_crash_handler() {
    std::signal(SIGABRT, crash_handler);
    std::signal(SIGSEGV, crash_handler);
    std::signal(SIGILL, crash_handler);
}

// Forward declare the buckyball extension factory
// This is defined in rocc.cc via REGISTER_EXTENSION macro
extern std::function<extension_t*()> buckyball_extension_factory;

// Manually register buckyball extension to avoid dlopen() issues
static void ensure_buckyball_registered() {
    static std::once_flag once;
    std::call_once(once, []() {
        register_extension("buckyball", buckyball_extension_factory);
    });
}

extern "C" {

struct spike_context_t {
    BTIF* btif = nullptr;
    processor_t* proc = nullptr;
    FILE* log_file = nullptr;
    state_t* state = nullptr;
    void* emu_state = nullptr;
    uint8_t* mem_ptr = nullptr;
    size_t mem_size = 0;
    uint64_t step_count = 0;
    uint64_t step_elapsed_ns = 0;
    bool profile_enabled = false;
    reg_t prev_pc = 0;
    bool pk_mode = false;
    bool finished = false;
    int exit_code = 0;
};

static void destroy_context(spike_context_t* ctx) {
    if (ctx == nullptr) {
        return;
    }
    if (ctx->log_file) {
        fclose(ctx->log_file);
    }
    delete ctx->proc;
    delete ctx->btif;
    delete ctx;
}

//===----------------------------------------------------------------------===//
// Initialize Stage 1: Initialize a new Spike instance
//===----------------------------------------------------------------------===//
static bool init_log(spike_context_t* ctx, const char* log_path) {
    if (log_path == nullptr) {
        return true;
    }
    ctx->log_file = fopen(log_path, "w");
    if (ctx->log_file != nullptr) {
        return true;
    }

    fprintf(stderr, "[ERROR] failed to open Spike log file: %s\n", log_path);
    fflush(stderr);
    return false;
}

static bool check_buckyball_mounted(spike_context_t* ctx) {
    if (ctx->proc->get_extension("buckyball") != nullptr) {
        return true;
    }

    fprintf(stderr, "[ERROR] buckyball extension not mounted\n");
    fflush(stderr);
    return false;
}

static void init_csrs(spike_context_t* ctx) {
    // Enable the floating-point unit for guest code. RISC-V gates floating-point
    // register/instruction use through mstatus.FS; leaving FS=Off makes ordinary
    // floating-point instructions trap even when the ISA string includes F/D.
    // Marking FS Dirty is the simplest functional-model setup for BEMU.
    constexpr reg_t MSTATUS_FS_MASK = 0x6000;
    constexpr reg_t MSTATUS_FS_DIRTY = 0x6000;
    reg_t mstatus = ctx->state->csrmap[CSR_MSTATUS]->read();
    mstatus = (mstatus & ~MSTATUS_FS_MASK) | MSTATUS_FS_DIRTY;
    ctx->state->csrmap[CSR_MSTATUS]->write(mstatus);

    // Allow S-mode/U-mode software to read the architectural counters used by
    // common runtimes and benchmarks: cycle, time, and instret. Without these
    // mcounteren/scounteren bits, instructions like rdcycle or rdtime trap
    // instead of returning counter values.
    constexpr reg_t COUNTEREN_MASK = 0x7;
    ctx->state->csrmap[CSR_MCOUNTEREN]->write(COUNTEREN_MASK);
    ctx->state->csrmap[CSR_SCOUNTEREN]->write(COUNTEREN_MASK);
}

void* spike_create_raw(
    const char* isa,
    size_t procs,
    size_t hart_id,
    uint8_t* mem_ptr,
    size_t mem_size,
    const char* log_path,
    uint8_t* uart_ptr,
    void* emu_state,
    bool profile_enabled
) {
    init_crash_handler();
    ensure_buckyball_registered();

    if (procs != 1) {
        fprintf(stderr, "[ERROR] only one Spike hart is supported for now, got %zu\n", procs);
        fflush(stderr);
        return nullptr;
    }

    auto* ctx = new spike_context_t();
    ctx->mem_ptr = mem_ptr;
    ctx->mem_size = mem_size;
    ctx->emu_state = emu_state;
    ctx->profile_enabled = profile_enabled;
    current_emu_state = ctx->emu_state;

    if (!init_log(ctx, log_path)) {
        destroy_context(ctx);
        return nullptr;
    }

    ctx->btif = new BTIF(mem_ptr, mem_size, uart_ptr, isa, hart_id);
    const char* final_isa = ctx->btif->get_cfg().isa;
    ctx->proc = new processor_t(final_isa, "MSU", &ctx->btif->get_cfg(), ctx->btif, hart_id, false, ctx->log_file, std::cerr);
    ctx->proc->reset();

    if (!check_buckyball_mounted(ctx)) {
        destroy_context(ctx);
        return nullptr;
    }

    ctx->proc->set_debug(ctx->log_file != nullptr);
    ctx->state = ctx->proc->get_state();

    // Initialize the CSRs for the guest code.
    init_csrs(ctx);

    return ctx;
}

//===----------------------------------------------------------------------===//
// Initialize Stage 2: Initialize hart
// 
// We can't init these before elf loading: trap_handler_addr, initial_sp, 
// initial_a0, initial_a1, initial_a2, tp_value_ptr, entry comes from the 
// ELF file, which is loaded by bemu.
//===----------------------------------------------------------------------===//
bool spike_init_hart_raw(
    void* raw_ctx,
    uint64_t entry,
    uint64_t trap_handler_addr,
    uint64_t satp,
    uint64_t initial_sp,
    uint64_t initial_a0,
    uint64_t initial_a1,
    uint64_t initial_a2,
    const uint64_t* tp_value_ptr,
    bool pk
) {
    auto* ctx = reinterpret_cast<spike_context_t*>(raw_ctx);
    if (ctx == nullptr) {
        return false;
    }
    current_emu_state = ctx->emu_state;
    ctx->pk_mode = pk;

    if (pk) {
        ctx->proc->set_max_vaddr_bits(39);
        ctx->state->csrmap[CSR_MTVEC]->write(trap_handler_addr);
        ctx->state->csrmap[CSR_SATP]->write(satp);
        ctx->proc->get_mmu()->flush_tlb();
        ctx->proc->set_privilege(PRV_U, false);
    }

    ctx->state->pc = entry;
    if (initial_sp != 0) {
        ctx->state->XPR.write(2, initial_sp);
    }
    ctx->state->XPR.write(10, initial_a0);
    ctx->state->XPR.write(11, initial_a1);
    ctx->state->XPR.write(12, initial_a2);
    if (tp_value_ptr != nullptr) {
        ctx->state->XPR.write(4, *tp_value_ptr);
    }
    ctx->prev_pc = ctx->state->pc;
    ctx->finished = false;
    ctx->exit_code = 0;
    ctx->step_count = 0;
    ctx->step_elapsed_ns = 0;
    ctx->btif->exit_requested = false;
    ctx->btif->exit_code = 0;
    return true;
}

//===----------------------------------------------------------------------===//
// Execute Stage: Execute step by step
//
//===----------------------------------------------------------------------===//
static spike_context_t* enter_context(void* raw_ctx) {
    auto* ctx = reinterpret_cast<spike_context_t*>(raw_ctx);
    if (ctx != nullptr) {
        current_emu_state = ctx->emu_state;
    }
    return ctx;
}

static bool finish_if_requested(spike_context_t* ctx) {
    if (ctx->finished || ctx->btif->exit_requested || should_exit_ffi(ctx->emu_state)) {
        ctx->finished = true;
        ctx->exit_code = ctx->btif->exit_requested ? ctx->btif->exit_code : get_exit_code_ffi(ctx->emu_state);
        return true;
    }
    return false;
}

static bool is_ecall_cause(reg_t cause) {
    return cause == CAUSE_USER_ECALL ||
           cause == CAUSE_SUPERVISOR_ECALL ||
           cause == CAUSE_MACHINE_ECALL;
}

static uint64_t handle_guest_syscall(spike_context_t* ctx) {
    return handle_syscall_ffi(
        ctx->emu_state,
        ctx->state->XPR[17],
        ctx->state->XPR[10],
        ctx->state->XPR[11],
        ctx->state->XPR[12],
        ctx->state->XPR[13],
        ctx->state->XPR[14],
        ctx->state->XPR[15]);
}

static int handle_syscall_magic_pc(spike_context_t* ctx) {
    const uint64_t SYSCALL_MAGIC_ADDR = DRAM_BASE + ctx->mem_size - 0x1000;
    if (ctx->state->pc != SYSCALL_MAGIC_ADDR) {
        return 0;
    }

    reg_t mcause = ctx->state->csrmap[CSR_MCAUSE]->read();
    if (!is_ecall_cause(mcause)) {
        fprintf(stderr,
                "[ERROR] Non-ecall trap reached syscall handler: mcause=%ld mepc=0x%lx tval=0x%lx pc=0x%lx tp=0x%lx a0=0x%lx a5=0x%lx\n",
                mcause, ctx->state->csrmap[CSR_MEPC]->read(), ctx->state->csrmap[CSR_MTVAL]->read(), ctx->state->pc,
                ctx->state->XPR[4], ctx->state->XPR[10], ctx->state->XPR[15]);
        fflush(stderr);
        ctx->finished = true;
        ctx->exit_code = 1;
        return -1;
    }

    ctx->state->XPR.write(10, handle_guest_syscall(ctx));
    ctx->state->pc = ctx->state->csrmap[CSR_MEPC]->read() + 4;
    if (ctx->pk_mode) {
        ctx->proc->set_privilege(PRV_U, false);
    } else {
        ctx->state->prv = PRV_S;
    }
    ctx->prev_pc = ctx->state->pc;
    return 1;
}

static bool skip_invalid_compressed_jump(spike_context_t* ctx) {
    uint16_t insn16 = 0;
    if (ctx->state->pc < DRAM_BASE || ctx->state->pc + 2 > DRAM_BASE + ctx->mem_size) {
        return false;
    }

    uint64_t off = ctx->state->pc - DRAM_BASE;
    memcpy(&insn16, ctx->mem_ptr + off, sizeof(insn16));
    if ((insn16 & 0xF07F) != 0x9002) {
        return false;
    }

    uint32_t rs1 = (insn16 >> 7) & 0x1F;
    reg_t target = ctx->state->XPR[rs1] & ~((reg_t)1);
    bool valid_high = target >= DRAM_BASE && target < DRAM_BASE + ctx->mem_size;
    if (valid_high) {
        return false;
    }

    ctx->state->pc += 2;
    ctx->prev_pc = ctx->state->pc;
    return true;
}

static reg_t trap_epc(spike_context_t* ctx, trap_t& trap) {
    if (trap.cause() == CAUSE_MACHINE_ECALL) {
        return ctx->state->csrmap[CSR_MEPC]->read();
    }
    if (trap.cause() == CAUSE_SUPERVISOR_ECALL) {
        return ctx->state->csrmap[CSR_SEPC]->read();
    }
    return ctx->state->pc;
}

static int fail_unhandled_trap(spike_context_t* ctx, trap_t& trap) {
    fprintf(stderr,
            "[ERROR] Unhandled trap: cause=%ld mcause=%ld mepc=0x%lx tval=0x%lx pc=0x%lx\n",
            trap.cause(), ctx->state->csrmap[CSR_MCAUSE]->read(), ctx->state->csrmap[CSR_MEPC]->read(),
            ctx->state->csrmap[CSR_MTVAL]->read(), ctx->state->pc);
    fflush(stderr);
    ctx->finished = true;
    ctx->exit_code = 1;
    return -1;
}

static int handle_trap(spike_context_t* ctx, trap_t& trap) {
    if (is_ecall_cause(trap.cause())) {
        ctx->state->XPR.write(10, handle_guest_syscall(ctx));
        if (ctx->pk_mode) {
            ctx->proc->get_mmu()->flush_tlb();
        }
        ctx->state->pc = trap_epc(ctx, trap) + 4;
        if (ctx->pk_mode) {
            ctx->proc->set_privilege(PRV_U, false);
        }
        return 0;
    }

    if (trap.cause() == CAUSE_BREAKPOINT) {
        ctx->state->pc += 4;
        return 0;
    }

    if (trap.cause() == CAUSE_MISALIGNED_LOAD || trap.cause() == CAUSE_MISALIGNED_STORE) {
        return 0;
    }

    return fail_unhandled_trap(ctx, trap);
}

static int step_one_instruction(spike_context_t* ctx) {
    if (!ctx->profile_enabled) {
        int result = 0;
        try {
            ctx->proc->step(1);
            ctx->step_count++;
        } catch (trap_t& trap) {
            result = handle_trap(ctx, trap);
        }
        return result;
    }

    const auto started = std::chrono::steady_clock::now();
    int result = 0;
    try {
        ctx->proc->step(1);
        ctx->step_count++;
    } catch (trap_t& trap) {
        result = handle_trap(ctx, trap);
    }
    const auto elapsed = std::chrono::steady_clock::now() - started;
    ctx->step_elapsed_ns += std::chrono::duration_cast<std::chrono::nanoseconds>(elapsed).count();
    return result;
}

static bool pc_jumped_to_zero(spike_context_t* ctx) {
    if (ctx->state->pc != 0 || ctx->prev_pc == 0) {
        return false;
    }

    fprintf(stderr, "[ERROR] PC jumped to 0! Previous PC = 0x%lx, step = %lu\n", ctx->prev_pc, ctx->step_count);
    fflush(stderr);
    ctx->finished = true;
    ctx->exit_code = 1;
    return true;
}

int spike_step_raw(void* raw_ctx) {
    auto* ctx = enter_context(raw_ctx);
    if (ctx == nullptr) {
        return -1;
    }

    if (finish_if_requested(ctx)) {
        return 1;
    }

    int syscall_magic = handle_syscall_magic_pc(ctx);
    if (syscall_magic < 0) {
        return -1;
    }
    if (syscall_magic > 0) {
        return 0;
    }

    if (skip_invalid_compressed_jump(ctx)) {
        return 0;
    }

    int step_result = step_one_instruction(ctx);
    if (step_result < 0) {
        return -1;
    }

    if (pc_jumped_to_zero(ctx)) {
        return -1;
    }

    ctx->prev_pc = ctx->state->pc;

    if (finish_if_requested(ctx)) {
        return 1;
    }
    return 0;
}

//===----------------------------------------------------------------------===//
// Finish Stage: Finish the execution
//===----------------------------------------------------------------------===//
bool spike_finished_raw(void* raw_ctx) {
    auto* ctx = reinterpret_cast<spike_context_t*>(raw_ctx);
    if (ctx == nullptr) {
        return true;
    }
    return ctx->finished || ctx->btif->exit_requested || should_exit_ffi(ctx->emu_state);
}

int spike_exit_code_raw(void* raw_ctx) {
    auto* ctx = reinterpret_cast<spike_context_t*>(raw_ctx);
    if (ctx == nullptr) {
        return 1;
    }
    if (ctx->btif->exit_requested) {
        return ctx->btif->exit_code;
    }
    if (should_exit_ffi(ctx->emu_state)) {
        return get_exit_code_ffi(ctx->emu_state);
    }
    return ctx->exit_code;
}

void spike_stop_raw(void* raw_ctx, int code) {
    auto* ctx = reinterpret_cast<spike_context_t*>(raw_ctx);
    if (ctx == nullptr) {
        return;
    }
    ctx->finished = true;
    ctx->exit_code = code;
}

uint64_t spike_step_elapsed_ns_raw(void* raw_ctx) {
    auto* ctx = reinterpret_cast<spike_context_t*>(raw_ctx);
    return ctx == nullptr ? 0 : ctx->step_elapsed_ns;
}

void spike_destroy_raw(void* raw_ctx) {
    auto* ctx = reinterpret_cast<spike_context_t*>(raw_ctx);
    destroy_context(ctx);
}

}
