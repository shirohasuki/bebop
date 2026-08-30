//! Shared Rust contract for the backend-neutral rushB host ABI.

pub const FUNCT7_FENCE: u32 = 0;
pub const FUNCT7_MVOUT: u32 = 16;
pub const FUNCT7_MSET: u32 = 32;
pub const FUNCT7_MVIN: u32 = 33;
pub const FUNCT7_MVIN_MMIO: u32 = 35;
pub const ACCELERATOR_LOCAL_ID_BITS: u32 = 16;
pub const ACCELERATOR_LOCAL_ID_MASK: u32 = (1 << ACCELERATOR_LOCAL_ID_BITS) - 1;

pub const fn encode_accelerator_id(tile_id: u32, local_id: u32) -> Option<u32> {
    if tile_id > ACCELERATOR_LOCAL_ID_MASK || local_id > ACCELERATOR_LOCAL_ID_MASK {
        None
    } else {
        Some((tile_id << ACCELERATOR_LOCAL_ID_BITS) | local_id)
    }
}

pub const fn decode_accelerator_id(accelerator_id: u32) -> (u32, u32) {
    (
        accelerator_id >> ACCELERATOR_LOCAL_ID_BITS,
        accelerator_id & ACCELERATOR_LOCAL_ID_MASK,
    )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RushBSelection {
    pub accelerator_id: u32,
    pub chip_id: i32,
}

impl RushBSelection {
    pub const fn new(accelerator_id: u32, chip_id: i32) -> Self {
        Self {
            accelerator_id,
            chip_id,
        }
    }
}

pub const DEFAULT_SELECTION: RushBSelection = RushBSelection::new(0, 0);

#[cfg(test)]
mod tests {
    use super::{decode_accelerator_id, encode_accelerator_id};

    #[test]
    fn accelerator_id_round_trip() {
        for (tile_id, local_id, expected) in [
            (0, 0, 0),
            (0, 3, 3),
            (1, 0, 65_536),
            (3, 4, 196_612),
            (65_535, 65_535, u32::MAX),
        ] {
            assert_eq!(encode_accelerator_id(tile_id, local_id), Some(expected));
            assert_eq!(decode_accelerator_id(expected), (tile_id, local_id));
        }
    }

    #[test]
    fn accelerator_id_rejects_oversized_fields() {
        assert_eq!(encode_accelerator_id(65_536, 0), None);
        assert_eq!(encode_accelerator_id(0, 65_536), None);
    }
}
