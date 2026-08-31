pub type CommandId = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RushCommand {
    pub id: CommandId,
    pub core_id: u32,
    pub xs1: u64,
    pub xs2: u64,
    pub funct7: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitMode {
    Accepted,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RushEventKind {
    Accepted,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RushEvent {
    pub command_id: CommandId,
    pub core_id: u32,
    pub kind: RushEventKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmaChunk {
    pub offset: usize,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum DmaOperation {
    None,
    Mvin {
        spans: Vec<(usize, usize)>,
        chunks: Vec<DmaChunk>,
    },
    Mvout {
        spans: Vec<(usize, usize)>,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RushResponse {
    pub output: Vec<DmaChunk>,
}
