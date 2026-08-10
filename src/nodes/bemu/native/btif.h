#pragma once

#include "cfg.h"
#include "simif.h"
#include <cstddef>
#include <cstdint>
#include <map>
#include <string>

class processor_t;

constexpr uint64_t SIM_EXIT_ADDR = 0x60000000UL;
constexpr uint64_t DRAM_BASE = 0x80000000UL;
constexpr uint64_t UART_BASE = 0x60020000UL;
constexpr uint64_t UART_SIZE = 0x100UL;

// BEMU-Target Interface: the memory/MMIO surface that BEMU exposes to Spike.
class BTIF : public simif_t {
public:
    BTIF(uint8_t* mem_ptr, size_t mem_size, uint8_t* uart_ptr, const char* isa, size_t hart_id);

    char* addr_to_mem(reg_t addr) override;
    bool mmio_load(reg_t addr, size_t len, uint8_t* bytes) override;
    bool mmio_store(reg_t addr, size_t len, const uint8_t* bytes) override;
    void proc_reset(unsigned) override;
    const cfg_t& get_cfg() const override;
    const std::map<size_t, processor_t*>& get_harts() const override;
    const char* get_symbol(uint64_t) override;

    bool exit_requested = false;
    int exit_code = 0;

private:
    uint8_t* mem_ptr;
    size_t mem_size;
    uint8_t* uart_ptr;
    std::string isa_storage;
    cfg_t cfg;
};
