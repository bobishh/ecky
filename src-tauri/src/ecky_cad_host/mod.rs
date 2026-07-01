use std::collections::BTreeMap;
use std::marker::PhantomData;

pub mod direct_occt;
pub mod direct_occt_executor;
pub mod direct_occt_normalize;
pub mod direct_occt_runner;
pub mod direct_occt_runtime;
pub mod direct_occt_sdk;
#[cfg(test)]
pub(crate) mod native_parity_harness;
pub mod svg_profile;
pub mod text_profile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OpaqueHandle<Tag> {
    raw: u64,
    _marker: PhantomData<fn() -> Tag>,
}

impl<Tag> OpaqueHandle<Tag> {
    pub const fn new(raw: u64) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    pub const fn raw(self) -> u64 {
        self.raw
    }
}

macro_rules! opaque_tag {
    ($tag:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $tag;
    };
}

opaque_tag!(ProgramHandleTag);
opaque_tag!(PartHandleTag);
opaque_tag!(NodeHandleTag);
opaque_tag!(ShapeHandleTag);
opaque_tag!(SketchHandleTag);
opaque_tag!(PathHandleTag);

pub type CadProgramHandle = OpaqueHandle<ProgramHandleTag>;
pub type CadPartHandle = OpaqueHandle<PartHandleTag>;
pub type CadNodeHandle = OpaqueHandle<NodeHandleTag>;
pub type CadShapeHandle = OpaqueHandle<ShapeHandleTag>;
pub type CadSketchHandle = OpaqueHandle<SketchHandleTag>;
pub type CadPathHandle = OpaqueHandle<PathHandleTag>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleState {
    Pending,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HandleRecord<Tag, T> {
    pub handle: OpaqueHandle<Tag>,
    pub label: Option<String>,
    pub state: HandleState,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HandleArena<Tag, T> {
    next_raw: u64,
    entries: BTreeMap<u64, HandleRecord<Tag, T>>,
    _marker: PhantomData<fn() -> Tag>,
}

impl<Tag, T> Default for HandleArena<Tag, T> {
    fn default() -> Self {
        Self {
            next_raw: 1,
            entries: BTreeMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<Tag, T> HandleArena<Tag, T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, value: T) -> OpaqueHandle<Tag> {
        self.insert_with_state(None, HandleState::Ready, value)
    }

    pub fn insert_with_label(&mut self, label: impl Into<String>, value: T) -> OpaqueHandle<Tag> {
        self.insert_with_state(Some(label.into()), HandleState::Ready, value)
    }

    pub fn insert_pending(&mut self, label: Option<String>, value: T) -> OpaqueHandle<Tag> {
        self.insert_with_state(label, HandleState::Pending, value)
    }

    fn insert_with_state(
        &mut self,
        label: Option<String>,
        state: HandleState,
        value: T,
    ) -> OpaqueHandle<Tag> {
        let raw = self.next_raw;
        self.next_raw += 1;
        let handle = OpaqueHandle::<Tag>::new(raw);
        self.entries.insert(
            raw,
            HandleRecord {
                handle: OpaqueHandle::<Tag>::new(raw),
                label,
                state,
                value,
            },
        );
        handle
    }

    pub fn get(&self, handle: OpaqueHandle<Tag>) -> Option<&T> {
        self.entries.get(&handle.raw()).map(|record| &record.value)
    }

    pub fn get_mut(&mut self, handle: OpaqueHandle<Tag>) -> Option<&mut T> {
        self.entries
            .get_mut(&handle.raw())
            .map(|record| &mut record.value)
    }

    pub fn get_record(&self, handle: OpaqueHandle<Tag>) -> Option<&HandleRecord<Tag, T>> {
        self.entries.get(&handle.raw())
    }

    pub fn remove(&mut self, handle: OpaqueHandle<Tag>) -> Option<HandleRecord<Tag, T>> {
        self.entries.remove(&handle.raw())
    }

    pub fn contains(&self, handle: OpaqueHandle<Tag>) -> bool {
        self.entries.contains_key(&handle.raw())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub type CadProgramArena<T> = HandleArena<ProgramHandleTag, T>;
pub type CadPartArena<T> = HandleArena<PartHandleTag, T>;
pub type CadNodeArena<T> = HandleArena<NodeHandleTag, T>;
pub type CadShapeArena<T> = HandleArena<ShapeHandleTag, T>;
pub type CadSketchArena<T> = HandleArena<SketchHandleTag, T>;
pub type CadPathArena<T> = HandleArena<PathHandleTag, T>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_allocates_monotonic_handles() {
        let mut arena: CadNodeArena<&str> = HandleArena::new();
        let first = arena.insert("alpha");
        let second = arena.insert("beta");
        assert_ne!(first, second);
        assert_eq!(first.raw(), 1);
        assert_eq!(second.raw(), 2);
        assert_eq!(arena.get(first), Some(&"alpha"));
        assert_eq!(arena.get(second), Some(&"beta"));
    }

    #[test]
    fn arena_tracks_label_and_state() {
        let mut arena: CadShapeArena<i32> = HandleArena::new();
        let handle = arena.insert_with_label("body", 17);
        let record = arena.get_record(handle).expect("record");
        assert_eq!(record.label.as_deref(), Some("body"));
        assert_eq!(record.state, HandleState::Ready);
        assert_eq!(record.value, 17);
    }

    #[test]
    fn pending_inserts_keep_state() {
        let mut arena: CadPartArena<&str> = HandleArena::new();
        let handle = arena.insert_pending(Some("mesh".into()), "queued");
        let record = arena.get_record(handle).expect("record");
        assert_eq!(record.state, HandleState::Pending);
        assert_eq!(record.label.as_deref(), Some("mesh"));
        assert_eq!(record.value, "queued");
    }

    #[test]
    fn typed_handles_keep_raw_identity() {
        let handle: CadShapeHandle = OpaqueHandle::new(99);
        assert_eq!(handle.raw(), 99);
    }
}
