#![feature(ptr_internals)]
#![feature(alloc_internals)]
use std::alloc::{self, Layout};
use std::iter::{DoubleEndedIterator, IntoIterator, Iterator};
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::ptr::{self, Unique};

struct RawVec<T> {
    ptr: Unique<T>,
    cap: usize,
}

impl<T> Drop for RawVec<T> {
    fn drop(&mut self) {
        if self.cap != 0 {
            let layout = Layout::array::<T>(self.cap).unwrap();
            unsafe {
                alloc::dealloc(self.ptr.as_ptr() as *mut _, layout);
            }
        }
    }
}

impl<T> RawVec<T> {
    pub fn new() -> Self {
        if mem::size_of::<T>() == 0 {
            unimplemented!("ZST is unsupported")
        }
        Self {
            ptr: Unique::dangling(),
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
}

struct Vec<T> {
    buf: RawVec<T>,
    len: usize,
}

impl<T> Vec<T> {
    pub fn new() -> Self {
        Self {
            buf: RawVec::new(),
            len: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buf.cap
    }

    pub fn push(&mut self, elem: T) {
        if self.buf.cap == self.len {
            self.buf.grow()
        }
        unsafe { ptr::write(self.buf.ptr.as_ptr().offset(self.len as isize), elem) };
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(ptr::read(self.buf.ptr.as_ptr().add(self.len))) }
        }
    }

    pub fn insert(&mut self, index: usize, elem: T) {
        assert!(index <= self.len, "index out of bounds");
        if self.len == self.buf.cap {
            self.buf.grow()
        }
        let p = self.buf.ptr.as_ptr();
        unsafe {
            if index < self.len {
                ptr::copy(p.add(index), p.add(index + 1), self.len - index);
            }
            ptr::write(p.add(index), elem);
            self.len += 1;
        }
    }
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "index out of bounds");
        unsafe {
            self.len -= 1;
            let p = self.buf.ptr.as_ptr();
            let elem = ptr::read(p.add(index));
            ptr::copy(p.add(index + 1), p.add(index), self.len - index);
            elem
        }
    }
}

impl<T> Drop for Vec<T> {
    fn drop(&mut self) {
        if self.buf.cap != 0 {
            // LLVM is smart enough to optimize the below if `T: !Drop`
            while let Some(_) = self.pop() {}
            // RawVec will dealloc the heap
        }
    }
}

impl<T> Deref for Vec<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        unsafe { std::slice::from_raw_parts(self.buf.ptr.as_ptr(), self.len) }
    }
}

impl<T> DerefMut for Vec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { std::slice::from_raw_parts_mut(self.buf.ptr.as_ptr(), self.len) }
    }
}

struct RawIter<T> {
    start: *const T,
    end: *const T,
}

impl<T> RawIter<T> {
    unsafe fn new(slice: &[T]) -> Self {
        let start = slice.as_ptr();
        let end = start.add(slice.len());
        Self { start, end }
    }
}

impl<T> DoubleEndedIterator for RawIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                self.end = self.end.sub(1);
                let ret = ptr::read(self.end);
                Some(ret)
            }
        }
    }
}

impl<T> Iterator for RawIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                let ret = ptr::read(self.start);
                self.start = self.start.add(1);
                Some(ret)
            }
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = (self.end as usize - self.start as usize) / mem::size_of::<T>();
        (len, Some(len))
    }
}

struct IntoIter<T> {
    _buf: RawVec<T>, // just holds the ownership
    iter: RawIter<T>,
}

impl<T> IntoIterator for Vec<T> {
    type IntoIter = IntoIter<T>;
    type Item = T;
    fn into_iter(self) -> Self::IntoIter {
        // Destruction calls `Vec::drop`, so unsafe read is required
        unsafe {
            let iter = RawIter::new(&self);
            let _buf = ptr::read(&self.buf);
            // To prevent compiler to call `drop` for each elements
            mem::forget(self);

            Self::IntoIter { _buf, iter }
        }
    }
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        for _ in &mut self.iter {}
    }
}

struct Drain<'a, T: 'a> {
    vec: PhantomData<&'a mut Vec<T>>,
    iter: RawIter<T>,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T> DoubleEndedIterator for Drain<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

impl<'a, T> Drop for Drain<'a, T> {
    fn drop(&mut self) {
        for _ in &mut self.iter {}
    }
}

impl<T> Vec<T> {
    pub fn drain<'a>(&'a mut self) -> Drain<'a, T> {
        unsafe {
            let iter = RawIter::new(&self);
            self.len = 0;
            Drain {
                vec: PhantomData,
                iter,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn new_vec(n: usize) -> Vec<Box<usize>> {
        let mut a = Vec::new();
        for i in 0..n {
            a.push(Box::new(i));
        }
        a
    }
    #[test]
    fn grow() {
        let mut a = RawVec::<usize>::new();
        a.grow();
        assert!(a.cap == 1);
        a.grow();
        assert!(a.cap == 2);
        println!("OK!");
    }
    #[test]
    fn push_pop() {
        let mut a = Vec::new();
        let n = 1000000;
        for i in 0..n {
            a.push(Box::new(i));
            unsafe {
                let e = ptr::read(a.buf.ptr.as_ptr().add(i));
                assert_eq!(*e, i);
                mem::forget(e);
            }
        }
        for i in (0..n).rev() {
            let e = a.pop().unwrap();
            assert_eq!(i, *e);
        }
        assert_eq!(a.pop(), None);
    }

    #[test]
    fn deref() {
        let mut a = Vec::<usize>::new();
        let n = 1000000;
        for i in 0..n {
            a.push(i);
            unsafe { assert_eq!(ptr::read(a.buf.ptr.as_ptr().add(i)), i) }
        }
        for (i, j) in a.iter().zip(0..n) {
            assert_eq!(*i, j)
        }
        for i in a.iter_mut() {
            *i *= 2;
        }
        for (i, j) in a.iter().zip(0..n) {
            assert_eq!(*i, j * 2)
        }
    }
    #[test]
    fn insert_remove() {
        let mut a = Vec::new();
        let n = 10000;
        for i in 0..n {
            a.insert(0, Box::new(i));
        }
        for (i, j) in (0..n).rev().zip(a.iter()) {
            assert_eq!(i, **j);
        }
        assert_eq!(*a.remove(n / 2), n / 2 - 1);
        for _ in 0..(n - 1) {
            a.remove(0);
        }
        assert_eq!(a.len, 0);
    }

    #[test]
    fn into_iter() {
        let n = 10000;
        let a = new_vec(n);
        for (i, j) in a.into_iter().zip(0..n) {
            assert_eq!(*i, j);
        }
    }

    #[test]
    fn double_ended_iterator() {
        let n = 10000;
        let a = new_vec(n);
        for (i, j) in a.into_iter().rev().zip((0..n).rev()) {
            assert_eq!(*i, j);
        }
    }
    #[test]
    fn drain() {
        let n = 10000;
        let mut a = new_vec(n);
        let b = a.drain();
        for (i, j) in b.zip(0..n) {
            assert_eq!(*i, j);
        }
        assert_eq!(a.len(), 0);
    }
}
