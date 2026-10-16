//! Capacity accounting, including spare capacity and conservative per-block slack.

use super::{bounded::*, dto::*, structure::StructuralSummary};
use std::mem::size_of;

pub(crate) const DOCUMENT_ALLOCATION_BYTES: usize = 4 * 1024 * 1024;

/// Observed Rust allocation requests, distinct from the conservative charge.
/// System heap metadata/fragmentation is not observable through safe collections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AllocationUsage {
    pub(crate) capacity_bytes: usize,
    pub(crate) blocks: usize,
    pub(crate) accounted_bytes: usize,
}

pub(super) fn add(a: usize, b: usize) -> Result<usize, String> {
    a.checked_add(b)
        .ok_or_else(|| "allocation accounting overflow".into())
}
pub(super) fn multiply(a: usize, b: usize) -> Result<usize, String> {
    a.checked_mul(b)
        .ok_or_else(|| "allocation accounting overflow".into())
}

// Policy, not an assertion about a Windows heap implementation: round each live
// request up to 64 bytes, then reserve another 64 bytes for allocator overhead.
pub(super) fn block_charge(bytes: usize) -> Result<usize, String> {
    if bytes == 0 {
        return Ok(0);
    }
    add(add(bytes, 63)? & !63, 64)
}

impl AllocationUsage {
    pub(super) fn block(&mut self, bytes: usize) -> Result<(), String> {
        if bytes != 0 {
            self.capacity_bytes = add(self.capacity_bytes, bytes)?;
            self.blocks = add(self.blocks, 1)?;
            self.accounted_bytes = add(self.accounted_bytes, block_charge(bytes)?)?;
        }
        Ok(())
    }
    pub(super) fn root<T>(&mut self) -> Result<(), String> {
        // Charge the root as though boxed, even when a caller keeps it on stack.
        // Headers of nested fields already live in this root or their parent Vec.
        self.accounted_bytes = add(self.accounted_bytes, block_charge(size_of::<T>())?)?;
        Ok(())
    }
    pub(super) fn check(&self) -> Result<(), String> {
        if self.accounted_bytes > DOCUMENT_ALLOCATION_BYTES {
            return Err("document accounted allocations exceed 4 MiB".into());
        }
        Ok(())
    }
}

/// Visits owned heap storage only; inline storage is charged by its owner once.
pub(super) trait Heap {
    fn heap(&self, usage: &mut AllocationUsage) -> Result<(), String>;
}
impl<T: Heap> Heap for Vec<T> {
    fn heap(&self, usage: &mut AllocationUsage) -> Result<(), String> {
        usage.block(multiply(self.capacity(), size_of::<T>())?)?;
        for item in self {
            item.heap(usage)?;
        }
        Ok(())
    }
}
impl<T: Heap, const N: usize> Heap for List<T, N> {
    fn heap(&self, usage: &mut AllocationUsage) -> Result<(), String> {
        self.0.heap(usage)
    }
}
impl<const NONEMPTY: bool> Heap for Text<NONEMPTY> {
    fn heap(&self, usage: &mut AllocationUsage) -> Result<(), String> {
        usage.block(self.0.capacity())
    }
}
impl<T: Heap> Heap for Option<T> {
    fn heap(&self, usage: &mut AllocationUsage) -> Result<(), String> {
        if let Some(value) = self {
            value.heap(usage)?;
        }
        Ok(())
    }
}
macro_rules! inline {
    ($($ty:ty),+ $(,)?) => {$(impl Heap for $ty {
        fn heap(&self, _: &mut AllocationUsage) -> Result<(), String> { Ok(()) }
    })+};
}
inline!(
    bool,
    u8,
    u16,
    usize,
    Number,
    I64Decimal,
    U64Decimal,
    ScalarKind,
    CandidateKind,
    Purpose,
    Selector,
    Density,
    TimeAxisMode,
    Layout,
    V1PanelKind,
    V1ControlKind,
    Applicability,
    super::StaticAvailability
);

// Exhaustive patterns deliberately fail to compile when a DTO gains a field.
macro_rules! object {
    ($ty:ident { $($field:ident),+ $(,)? }) => {impl Heap for $ty {
        fn heap(&self, usage: &mut AllocationUsage) -> Result<(), String> {
            let Self { $($field),+ } = self;
            $($field.heap(usage)?;)+
            Ok(())
        }
    }};
}
macro_rules! union {
    ($ty:ident { $($variant:ident { $($field:ident),* $(,)? }),+ $(,)? }) => {impl Heap for $ty {
        fn heap(&self, usage: &mut AllocationUsage) -> Result<(), String> {
            match self { $(Self::$variant { $($field),* } => { $($field.heap(usage)?;)* }),+ }
            Ok(())
        }
    }};
}
object!(DocumentDto {
    format_version,
    binding_version,
    document_id,
    windows,
    components,
    data_bindings,
    editor_bindings,
    action_bindings,
    plots,
    settings
});
object!(Settings { density });
object!(Window { id, title, tabs });
object!(Tab { id, title, panels });
object!(Panel {
    id,
    title,
    components,
    v1_kind
});
object!(V1ControlOrigin {
    kind,
    target,
    confirm
});
object!(Plot {
    id,
    title,
    time_window_seconds,
    axes,
    traces
});
object!(Axes {
    y_min,
    y_max,
    time_mode
});
object!(Trace {
    id,
    data_id,
    display_label,
    visible,
    style,
    display_unit
});
object!(TraceStyle { color, width });
object!(DataBinding {
    id,
    selector,
    target,
    expect_kind
});
object!(EditorBinding {
    id,
    data_id,
    purpose,
    candidate_kind,
    constraints
});
object!(Constraints {
    integer_min,
    integer_max,
    number_min,
    number_max,
    max_utf8_bytes,
    choices
});
object!(Choice { token, label });
object!(StructuralSummary {
    expanded_components,
    expanded_plots,
    data,
    editors,
    actions
});
union!(Component {
    Group { id, label, help, layout, children, v1_control },
    Button { id, label, help, action_id, disabled_reason },
    Toggle { id, label, help, editor_id, applicability },
    IntegerInput { id, label, help, editor_id, applicability },
    NumberInput { id, label, help, editor_id, applicability },
    TextInput { id, label, help, editor_id, applicability },
    Select { id, label, help, editor_id, applicability },
    Indicator { id, label, help, data_id, precision },
    Status { id, label, help, source },
    Plot { id, label, help, plot_id },
});
union!(RuntimeRef {
    Instrument { instrument }, Signal { instrument, parameter }, Reference { reference },
    Controller { controller }, Resource { resource }, Component { component }, Recorder {},
    ConfigurationProperty { owner, property },
});
union!(ConfigurationOwner { Instrument { instrument }, Component { component }, Resource { resource } });
union!(StatusSource { Data { data_id }, Entity { target } });
union!(TypedScalar { Boolean { value }, Integer { value }, Number { value }, Text { value }, Enum { value } });
union!(Parameter { Editor { editor_id }, Literal { value } });
union!(ActionBinding {
    RecorderLifecycle { id, target, label }, ReferenceFixed { id, target, value },
    ControllerLifecycle { id, target }, PropertyConfigure { id, target, value },
});
