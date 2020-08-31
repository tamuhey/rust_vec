#![feature(ptr_internals)]
#![feature(alloc_internals)]
use std::alloc::{self, Layout};
use std::marker::PhantomData;
use std::mem;
use std::ptr::{self, Unique};

struct Vec<T> {
    ptr: Unique<T>,
    cap: usize,
    len: usize,
}

impl<T> Vec<T> {
    fn new() -> Self {
        if mem::size_of::<T>() == 0 {
            unimplemented!("ZST is unsupported")
        }
        Self {
            ptr: Unique::dangling(),
            len: 0,
            cap: 0,
        }
    }
    fn grow(&mut self) {
        unsafe {
            let layout = Layout::new::<T>();
            let (new_cap, new_ptr) = if self.cap == 0 {
                (1, alloc::alloc(layout))
            } else {
                let new_cap = self.cap * 2;
                let new_layout = Layout::array::<T>(new_cap).unwrap();

                if new_layout.size() >= std::isize::MAX as usize {
                    // Since LLVM doesn't have unsigned integer type, the allowed maximum usize is isize:MAX
                    panic!("capacity overflow");
                }
                let new_ptr =
                    alloc::realloc(self.ptr.as_ptr() as *mut _, layout, new_layout.size());
                if new_ptr.is_null() {
                    alloc::rust_oom(layout);
                }
                (new_cap, new_ptr)
            };
            self.ptr = Unique::new(new_ptr as *mut T).unwrap();
            self.cap = new_cap;
        }
    }
    fn push(&mut self, elem: T) {
        if self.cap == self.len {
            self.grow()
        }
        unsafe { ptr::write(self.ptr.as_ptr().offset(self.len as isize), elem) };
        self.len += 1;
    }
    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(ptr::read(self.ptr.as_ptr().add(self.len))) }
        }
    }
}

impl<T> Drop for Vec<T> {
    fn drop(&mut self) {
        if self.cap != 0 {
            let layout = Layout::array::<T>(self.cap).unwrap();

            // LLVM is smart enough to optimize the below if `T: !Drop`
            while let Some(_) = self.pop() {}
            unsafe {
                alloc::dealloc(self.ptr.as_ptr() as *mut _, layout);
            }
        }
    }
}

fn main() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn main() {
        let mut a = Vec::<usize>::new();
        a.grow();
        assert!(a.cap == 1);
        a.grow();
        assert!(a.cap == 2);
        println!("OK!");
    }
    #[test]
    fn push_pop() {
        let mut a = Vec::<usize>::new();
        let n = 1000000;
        for i in 0..n {
            a.push(i);
            unsafe { assert_eq!(ptr::read(a.ptr.as_ptr().add(i)), i) }
        }
        for i in (0..n).rev() {
            let e = a.pop().unwrap();
            assert_eq!(i, e);
        }
        assert_eq!(a.pop(), None);
    }
}
