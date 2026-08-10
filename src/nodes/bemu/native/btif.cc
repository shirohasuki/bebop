//===----- btif.cc -------- bemu target interface --------------------===//
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
// BTIF (BEMU-Target Interface): the surface that BEMU exposes to Spike.
// Currently, it supports serval class of functionalities as follows:
//  1. Memory management: spike doesn't manage its own memory, it uses the 
//     memory provided by bemu.
//  2. Utilities: spike constracts simif_t to provide a unified interface for
//     spike execution.
//===----------------------------------------------------------------------===//


#include "btif.h"
#include <cstring>

extern "C" {
    uint64_t uart_mmio_load(uint8_t* uart_ptr, uint64_t addr, size_t size);
    bool uart_mmio_store(uint8_t* uart_ptr, uint64_t addr, size_t size, uint64_t value);
}

BTIF::BTIF(uint8_t* mem_ptr, size_t mem_size, uint8_t* uart_ptr, const char* isa, size_t hart_id)
    : mem_ptr(mem_ptr), mem_size(mem_size), uart_ptr(uart_ptr) {
    isa_storage = isa;
    cfg.isa = isa_storage.c_str();
    cfg.priv = "MSU";
    cfg.mem_layout.push_back(mem_cfg_t(DRAM_BASE, mem_size));
    cfg.hartids.push_back(hart_id);
}

char* BTIF::addr_to_mem(reg_t addr) {
    if (addr >= DRAM_BASE && addr < DRAM_BASE + mem_size) {
        return reinterpret_cast<char*>(mem_ptr + (addr - DRAM_BASE));
    }
    return nullptr;
}

bool BTIF::mmio_load(reg_t addr, size_t len, uint8_t* bytes) {
    if (addr >= UART_BASE && addr < UART_BASE + UART_SIZE) {
        uint64_t value = uart_mmio_load(uart_ptr, addr, len);
        memcpy(bytes, &value, len);
        return true;
    }
    return false;
}

bool BTIF::mmio_store(reg_t addr, size_t len, const uint8_t* bytes) {
    if (addr == SIM_EXIT_ADDR) {
        uint64_t value = 0;
        memcpy(&value, bytes, len);
        exit_code = static_cast<int>(value & 0xffffffff);
        exit_requested = true;
        return true;
    }

    if (addr >= UART_BASE && addr < UART_BASE + UART_SIZE) {
        uint64_t value = 0;
        memcpy(&value, bytes, len);
        return uart_mmio_store(uart_ptr, addr, len, value);
    }

    return false;
}

void BTIF::proc_reset(unsigned) {}

const cfg_t& BTIF::get_cfg() const {
    return cfg;
}

const std::map<size_t, processor_t*>& BTIF::get_harts() const {
    static std::map<size_t, processor_t*> empty_map;
    return empty_map;
}

const char* BTIF::get_symbol(uint64_t) {
    return nullptr;
}
