//! Private framing shared with the no-GPU child fixture. Version 2 adds only
//! closed CONTROL-1 intent/feedback, never arbitrary Application commands.

pub(crate) const VERSION: u16 = 2;
pub(crate) const HEADER: usize = 96;
pub(crate) const CONTROL: usize = 4096;
pub(crate) const CHUNK: usize = 65536;
pub(crate) const SNAPSHOT: usize = 1048576;
pub(crate) const SLOTS: usize = 4;
pub(crate) const TRACES: usize = 32;
pub(crate) const POINTS: usize = 16384;
pub(crate) const DIAGNOSTICS: usize = 64;
pub(crate) const DIAGNOSTIC: usize = 1024;
pub(crate) const OWNER_MESSAGES: usize = 4;
pub(crate) const OWNER_BYTES: usize = 65536;
pub(crate) const ENCODED_STORAGE: usize =
    SLOTS * SNAPSHOT + 2 * CHUNK + 2 * CONTROL + DIAGNOSTICS * DIAGNOSTIC;
pub(crate) type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum Kind {
    Hello = 1,
    HelloAck,
    Challenge,
    Status,
    Close,
    Closed,
    Diagnostic,
    Fence,
    Skipped,
    WindowClose,
    ControlIntent,
    ControlFeedback,
    Document = 16,
    Observations,
    Applied,
}
impl Kind {
    fn decode(value: u16) -> Result<Self> {
        Ok(match value {
            1 => Self::Hello,
            2 => Self::HelloAck,
            3 => Self::Challenge,
            4 => Self::Status,
            5 => Self::Close,
            6 => Self::Closed,
            7 => Self::Diagnostic,
            8 => Self::Fence,
            9 => Self::Skipped,
            10 => Self::WindowClose,
            11 => Self::ControlIntent,
            12 => Self::ControlFeedback,
            16 => Self::Document,
            17 => Self::Observations,
            18 => Self::Applied,
            _ => return Err("unknown private variant"),
        })
    }
    pub(crate) fn stream(self) -> Option<usize> {
        match self {
            Self::Document => Some(0),
            Self::Observations => Some(1),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) workbench: [u8; 16],
    pub(crate) incarnation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Epoch {
    pub(crate) owner: [u8; 16],
    pub(crate) revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) kind: Kind,
    pub(crate) bytes: usize,
    pub(crate) total: usize,
    pub(crate) identity: Identity,
    pub(crate) sequence: u64,
    pub(crate) epoch: Epoch,
    pub(crate) snapshot: u64,
    pub(crate) offset: usize,
    // Source generation is a parent projection fence, never a Runtime sequence.
    // Exact Runtime boot/source/revision metadata lives in the read payload.
    pub(crate) source: u64,
}
impl Header {
    pub(crate) fn control(kind: Kind, identity: Identity, sequence: u64) -> Self {
        Self {
            kind,
            bytes: HEADER,
            total: 0,
            identity,
            sequence,
            epoch: Epoch::default(),
            snapshot: 0,
            offset: 0,
            source: 0,
        }
    }
    pub(crate) fn encode(self, out: &mut [u8]) -> Result<()> {
        if out.len() < HEADER {
            return Err("short header buffer");
        }
        let length = u32::try_from(self.bytes).map_err(|_| "header length overflow")?;
        let total = u32::try_from(self.total).map_err(|_| "header total overflow")?;
        let offset = u32::try_from(self.offset).map_err(|_| "header offset overflow")?;
        out[..HEADER].fill(0);
        out[..4].copy_from_slice(b"LIPC");
        out[4..6].copy_from_slice(&VERSION.to_le_bytes());
        out[6..8].copy_from_slice(&(self.kind as u16).to_le_bytes());
        out[8..12].copy_from_slice(&length.to_le_bytes());
        out[12..16].copy_from_slice(&total.to_le_bytes());
        out[16..32].copy_from_slice(&self.identity.workbench);
        out[32..40].copy_from_slice(&self.identity.incarnation.to_le_bytes());
        out[40..48].copy_from_slice(&self.sequence.to_le_bytes());
        out[48..64].copy_from_slice(&self.epoch.owner);
        out[64..72].copy_from_slice(&self.epoch.revision.to_le_bytes());
        out[72..80].copy_from_slice(&self.snapshot.to_le_bytes());
        out[80..84].copy_from_slice(&offset.to_le_bytes());
        out[88..96].copy_from_slice(&self.source.to_le_bytes());
        Ok(())
    }
    /// All declared lengths checked before a receiver reserves a slot or reads a body.
    pub(crate) fn decode(bytes: &[u8], identity: Identity, snapshot_channel: bool) -> Result<Self> {
        if bytes.len() != HEADER || &bytes[..4] != b"LIPC" || bytes[84..88] != [0; 4] {
            return Err("malformed private header");
        }
        let u16_at = |n| u16::from_le_bytes(bytes[n..n + 2].try_into().unwrap());
        let u32_at = |n| u32::from_le_bytes(bytes[n..n + 4].try_into().unwrap()) as usize;
        let u64_at = |n| u64::from_le_bytes(bytes[n..n + 8].try_into().unwrap());
        if u16_at(4) != VERSION {
            return Err("unsupported private version");
        }
        let kind = Kind::decode(u16_at(6))?;
        let actual = Identity {
            workbench: bytes[16..32].try_into().unwrap(),
            incarnation: u64_at(32),
        };
        if actual != identity || actual.incarnation == 0 {
            return Err("wrong private identity/incarnation");
        }
        let result = Self {
            kind,
            bytes: u32_at(8),
            total: u32_at(12),
            identity: actual,
            sequence: u64_at(40),
            epoch: Epoch {
                owner: bytes[48..64].try_into().unwrap(),
                revision: u64_at(64),
            },
            snapshot: u64_at(72),
            offset: u32_at(80),
            source: u64_at(88),
        };
        if result.sequence == 0
            || result.bytes < HEADER
            || result.bytes > if snapshot_channel { CHUNK } else { CONTROL }
        {
            return Err("private frame bound/sequence");
        }
        if snapshot_channel != kind.stream().is_some() {
            return Err("wrong private channel");
        }
        if snapshot_channel {
            if result.total == 0
                || result.total > SNAPSHOT
                || result.snapshot == 0
                || result.epoch.revision == 0
                || result.epoch.owner == [0; 16]
                || result.bytes == HEADER
                || result
                    .offset
                    .checked_add(result.bytes - HEADER)
                    .is_none_or(|n| n > result.total)
            {
                return Err("snapshot bounds/epoch");
            }
        } else if result.total != 0
            || result.offset != 0
            || (!matches!(kind, Kind::Applied | Kind::Skipped)
                && (result.epoch != Epoch::default() || result.snapshot != 0 || result.source != 0))
        {
            return Err("noncanonical control metadata");
        }
        Ok(result)
    }
}

#[derive(Default)]
pub(crate) struct Sequence(u64);
impl Sequence {
    pub(crate) fn next(&mut self) -> Result<u64> {
        self.0 = self.0.checked_add(1).ok_or("private sequence exhausted")?;
        Ok(self.0)
    }
    pub(crate) fn accept(&mut self, value: u64) -> Result<()> {
        if self.0.checked_add(1) != Some(value) {
            return Err("duplicate/out-of-order private sequence");
        }
        self.0 = value;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Credit {
    Free,
    Latest(usize),
    Current(usize),
    Writing(usize),
    Assembly(usize),
}
pub(crate) struct Slot {
    pub(crate) bytes: Box<[u8]>,
    pub(crate) len: usize,
    pub(crate) credit: Credit,
    pub(crate) epoch: Epoch,
    pub(crate) snapshot: u64,
    pub(crate) source: u64,
}
pub(crate) struct Pool {
    pub(crate) slots: [Slot; SLOTS],
    pub(crate) coalesced: u64,
}
impl Pool {
    pub(crate) fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot {
                bytes: vec![0; SNAPSHOT].into_boxed_slice(),
                len: 0,
                credit: Credit::Free,
                epoch: Epoch::default(),
                snapshot: 0,
                source: 0,
            }),
            coalesced: 0,
        }
    }
    /// Fill only an unborrowed latest/free slot after complete immutable input
    /// admission. Numeric/identity rejection happens before any bytes change.
    pub(crate) fn publish(
        &mut self,
        stream: usize,
        epoch: Epoch,
        snapshot: u64,
        source: u64,
        len: usize,
        encode: impl FnOnce(&mut [u8]),
    ) -> Result<()> {
        if stream > 1
            || epoch.revision == 0
            || epoch.owner == [0; 16]
            || snapshot == 0
            || len == 0
            || len > SNAPSHOT
        {
            return Err("publication identity");
        }
        if self.slots.iter().any(|s| s.credit != Credit::Free && matches!(s.credit, Credit::Latest(k) | Credit::Current(k) | Credit::Writing(k) if k == stream) && (snapshot <= s.snapshot || s.epoch.owner != epoch.owner || s.epoch.revision > epoch.revision || s.source > source)) { return Err("stale publication"); }
        let old = self
            .slots
            .iter()
            .position(|s| s.credit == Credit::Latest(stream));
        let index = old
            .or_else(|| self.slots.iter().position(|s| s.credit == Credit::Free))
            .ok_or("encoded credits exhausted")?;
        let slot = &mut self.slots[index];
        encode(&mut slot.bytes[..len]);
        slot.len = len;
        slot.epoch = epoch;
        slot.snapshot = snapshot;
        slot.source = source;
        slot.credit = Credit::Latest(stream);
        if old.is_some() {
            self.coalesced = self.coalesced.saturating_add(1);
        }
        Ok(())
    }
    pub(crate) fn start_write(&mut self, stream: usize) -> Option<usize> {
        if self
            .slots
            .iter()
            .any(|s| s.credit == Credit::Writing(stream))
        {
            return None;
        }
        let index = self
            .slots
            .iter()
            .position(|s| s.credit == Credit::Latest(stream))?;
        // Current remains until replacement is transmitted. Its memory is not
        // kernel borrowed; release it to make room for at most one new latest.
        for slot in &mut self.slots {
            if slot.credit == Credit::Current(stream) {
                slot.credit = Credit::Free;
            }
        }
        self.slots[index].credit = Credit::Writing(stream);
        Some(index)
    }
    pub(crate) fn complete_write(&mut self, index: usize, stream: usize) -> Result<()> {
        if self.slots[index].credit != Credit::Writing(stream) {
            return Err("write credit mismatch");
        }
        self.slots[index].credit = Credit::Current(stream);
        Ok(())
    }
    pub(crate) fn restore(&mut self) -> Result<()> {
        if self
            .slots
            .iter()
            .any(|s| matches!(s.credit, Credit::Writing(_) | Credit::Assembly(_)))
        {
            return Err("unretired snapshot borrow");
        }
        for stream in 0..2 {
            let latest = self
                .slots
                .iter()
                .any(|s| s.credit == Credit::Latest(stream));
            for slot in &mut self.slots {
                if slot.credit == Credit::Current(stream) {
                    slot.credit = if latest {
                        Credit::Free
                    } else {
                        Credit::Latest(stream)
                    };
                }
            }
        }
        Ok(())
    }
}
