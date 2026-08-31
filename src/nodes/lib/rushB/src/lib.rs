//! Shared Rust contract for the backend-neutral rushB host ABI.

pub const FUNCT7_FENCE: u32 = 0;
pub const FUNCT7_MVOUT: u32 = 16;
pub const FUNCT7_MSET: u32 = 32;
pub const FUNCT7_MVIN: u32 = 33;
pub const FUNCT7_MVIN_MMIO: u32 = 35;
pub const CORE_LOCAL_ID_BITS: u32 = 16;
pub const CORE_LOCAL_ID_MASK: u32 = (1 << CORE_LOCAL_ID_BITS) - 1;

pub const fn encode_core_id(tile_id: u32, local_id: u32) -> Option<u32> {
    if tile_id > CORE_LOCAL_ID_MASK || local_id > CORE_LOCAL_ID_MASK {
        None
    } else {
        Some((tile_id << CORE_LOCAL_ID_BITS) | local_id)
    }
}

pub const fn decode_core_id(core_id: u32) -> (u32, u32) {
    (core_id >> CORE_LOCAL_ID_BITS, core_id & CORE_LOCAL_ID_MASK)
}

#[cfg(test)]
mod tests {
    use super::{decode_core_id, encode_core_id};

    #[test]
    fn core_id_round_trip() {
        for (tile_id, local_id, expected) in [
            (0, 0, 0),
            (0, 3, 3),
            (1, 0, 65_536),
            (3, 4, 196_612),
            (65_535, 65_535, u32::MAX),
        ] {
            assert_eq!(encode_core_id(tile_id, local_id), Some(expected));
            assert_eq!(decode_core_id(expected), (tile_id, local_id));
        }
    }

    #[test]
    fn core_id_rejects_oversized_fields() {
        assert_eq!(encode_core_id(65_536, 0), None);
        assert_eq!(encode_core_id(0, 65_536), None);
    }
}
