use std::alloc::{self, Layout};
use std::any::TypeId;
use std::ptr::NonNull;

use crate::component::ComponentId;

/// Metadata describing a registered component type.
pub struct ComponentInfo {
    /// Runtime ID for this component type.
    id: ComponentId,

    /// Rust `TypeId` for this component type.
    type_id: TypeId,

    /// Size and alignment information for this component type.
    layout: Layout,

    /// Function used to drop one initialized value of this component type.
    drop_fn: unsafe fn(*mut u8),
}

impl ComponentInfo {
    /// Creates metadata for a component type.
    pub fn new<T: 'static>(id: ComponentId) -> Self {
        let layout = Layout::new::<T>();
        assert!(
            layout.size() > 0,
            "zero-sized components are not supported yet"
        );

        Self {
            id,
            type_id: TypeId::of::<T>(),
            layout: Layout::new::<T>(),
            drop_fn: drop_ptr::<T>,
        }
    }

    /// Returns the runtime ID of this component type.
    pub fn id(&self) -> ComponentId {
        self.id
    }

    /// Returns the `TypeId` of this component type.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Returns the memory layout of this component type.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Returns the function used to drop one stored component value.
    pub fn drop_fn(&self) -> unsafe fn(*mut u8) {
        self.drop_fn
    }

    /// Returns the size of one component value in bytes.
    pub fn size(&self) -> usize {
        self.layout.size()
    }

    /// Returns the alignment of this component type in bytes.
    pub fn align(&self) -> usize {
        self.layout.align()
    }
}

/// Dense raw storage for component values of a single type.
///
/// A column stores only one component type, described by its `ComponentInfo`.
/// Values are tightly packed and indexed by row.
///
/// # Invariants
///
/// - `ptr` is allocated with alignment suitable for the component type.
/// - The first `len` elements are initialized.
/// - Elements in the range `len..capacity` are uninitialized.
/// - All stored values have the type described by `info`.
pub struct Column {
    info: ComponentInfo,
    ptr: NonNull<u8>,
    len: usize,
    capacity: usize,
}

impl Column {
    /// Creates an empty column for one component type.
    pub fn new(info: ComponentInfo) -> Self {
        Self {
            info,
            ptr: NonNull::dangling(),
            len: 0,
            capacity: 0,
        }
    }

    /// Returns metadata describing the component type stored in this column.
    pub fn info(&self) -> &ComponentInfo {
        &self.info
    }

    /// Returns the number of initialized component values in the column.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the column contains no values.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn assert_type<T: 'static>(&self) {
        assert_eq!(
            self.info.type_id(),
            TypeId::of::<T>(),
            "column type mismatch"
        );
    }

    fn layout_for_capacity(&self, capacity: usize) -> Layout {
        let size = self
            .info
            .size()
            .checked_mul(capacity)
            .expect("column capacity overflow");

        Layout::from_size_align(size, self.info.align()).expect("invalid column layout")
    }

    fn grow(&mut self) {
        let new_capacity = if self.capacity == 0 {
            4
        } else {
            self.capacity * 2
        };

        let new_layout = self.layout_for_capacity(new_capacity);

        unsafe {
            let new_ptr = if self.capacity == 0 {
                alloc::alloc(new_layout)
            } else {
                let old_layout = self.layout_for_capacity(self.capacity);
                alloc::realloc(self.ptr.as_ptr(), old_layout, new_layout.size())
            };

            self.ptr = NonNull::new(new_ptr).expect("allocation failed");
        }

        self.capacity = new_capacity;
    }

    fn element_ptr(&self, index: usize) -> *mut u8 {
        assert!(index < self.capacity);

        let offset = index
            .checked_mul(self.info.size())
            .expect("element offset overflow");

        unsafe { self.ptr.as_ptr().add(offset) }
    }

    /// Appends one component value to the column.
    pub fn push<T: 'static>(&mut self, value: T) {
        self.assert_type::<T>();

        if self.len == self.capacity {
            self.grow();
        }

        unsafe {
            self.element_ptr(self.len).cast::<T>().write(value);
        }

        self.len += 1;
    }

    /// Returns an immutable reference to the value at `index`.
    pub fn get<T: 'static>(&self, index: usize) -> Option<&T> {
        self.assert_type::<T>();

        if index >= self.len {
            return None;
        }

        unsafe { Some(&*self.element_ptr(index).cast::<T>()) }
    }

    /// Returns a mutable reference to the value at `index`.
    pub fn get_mut<T: 'static>(&mut self, index: usize) -> Option<&mut T> {
        self.assert_type::<T>();

        if index >= self.len {
            return None;
        }

        unsafe { Some(&mut *self.element_ptr(index).cast::<T>()) }
    }

    /// Removes and returns the value at `index` using swap-remove.
    ///
    /// This does not preserve order.
    pub fn swap_remove<T: 'static>(&mut self, index: usize) -> Option<T> {
        self.assert_type::<T>();

        if index >= self.len {
            return None;
        }

        let last_index = self.len - 1;

        unsafe {
            let removed = self.element_ptr(index).cast::<T>().read();

            if index != last_index {
                let last_value = self.element_ptr(last_index).cast::<T>().read();
                self.element_ptr(index).cast::<T>().write(last_value);
            }

            self.len -= 1;
            Some(removed)
        }
    }
}

impl Drop for Column {
    fn drop(&mut self) {
        unsafe {
            for index in 0..self.len {
                let ptr = self.element_ptr(index);
                (self.info.drop_fn())(ptr);
            }

            if self.capacity != 0 {
                let layout = self.layout_for_capacity(self.capacity);
                alloc::dealloc(self.ptr.as_ptr(), layout);
            }
        }
    }
}

/// Drops a value of type `T` stored at `ptr`.
///
/// # Safety
///
/// `ptr` must be valid, properly aligned for `T`, and point to an initialized
/// value of type `T`.
unsafe fn drop_ptr<T>(ptr: *mut u8) {
    unsafe {
        ptr.cast::<T>().drop_in_place();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[test]
    fn push_and_get_component() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });
        column.push(Position { x: 3.0, y: 4.0 });

        assert_eq!(column.len(), 2);
        assert_eq!(
            column.get::<Position>(0),
            Some(&Position { x: 1.0, y: 2.0 })
        );
        assert_eq!(
            column.get::<Position>(1),
            Some(&Position { x: 3.0, y: 4.0 })
        );
    }

    #[test]
    fn get_returns_none_out_of_bounds() {
        let info = ComponentInfo::new::<Position>(0);
        let column = Column::new(info);

        assert!(column.get::<Position>(0).is_none());
    }

    #[test]
    fn get_mut_allows_modification() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });

        let pos = column.get_mut::<Position>(0).unwrap();
        pos.x = 10.0;

        assert_eq!(column.get::<Position>(0).unwrap().x, 10.0);
    }

    #[test]
    fn swap_remove_removes_value() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });
        column.push(Position { x: 3.0, y: 4.0 });
        column.push(Position { x: 5.0, y: 6.0 });

        let removed = column.swap_remove::<Position>(1);

        assert_eq!(removed, Some(Position { x: 3.0, y: 4.0 }));
        assert_eq!(column.len(), 2);
        assert_eq!(
            column.get::<Position>(1),
            Some(&Position { x: 5.0, y: 6.0 })
        );
    }

    #[test]
    fn swap_remove_returns_none_out_of_bounds() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        assert!(column.swap_remove::<Position>(0).is_none());
    }

    #[test]
    fn dropping_column_drop_stored_values() {
        use std::cell::RefCell;
        use std::rc::Rc;

        struct Droppy {
            counter: Rc<RefCell<usize>>,
        }

        impl Drop for Droppy {
            fn drop(&mut self) {
                *self.counter.borrow_mut() += 1;
            }
        }

        let counter = Rc::new(RefCell::new(0));

        {
            let info = ComponentInfo::new::<Droppy>(0);
            let mut column = Column::new(info);

            column.push(Droppy {
                counter: counter.clone(),
            });
            column.push(Droppy {
                counter: counter.clone(),
            });
        }

        assert_eq!(*counter.borrow(), 2);
    }

    #[test]
    #[should_panic(expected = "column type mismatch")]
    fn get_with_wrong_type_panics() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(Position { x: 1.0, y: 2.0 });

        let _ = column.get::<i32>(0);
    }

    #[test]
    #[should_panic(expected = "column type mismatch")]
    fn push_with_wrong_type_panics() {
        let info = ComponentInfo::new::<Position>(0);
        let mut column = Column::new(info);

        column.push(123_i32);
    }

    #[test]
    #[should_panic(expected = "zero-sized components are not supported yet")]
    fn zero_sized_components_are_rejected() {
        struct Marker;

        let _ = ComponentInfo::new::<Marker>(0);
    }
}
