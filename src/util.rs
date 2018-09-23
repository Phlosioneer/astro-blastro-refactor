
use std::mem;
use std::cell::RefCell;

///////////////////////////////////////////////////////////////////////////////////
// This is a proposed addition to RefCell; for now, I've written an extension trait
// for the same functionality.
// https://github.com/rust-lang/rust/issues/54493
pub trait RefCellTryReplaceExt<T> {
    fn try_replace(&self, t: T) -> Result<T, ReplaceError>;
    fn try_swap(&self, other: &RefCell<T>) -> Result<(), SwapError>;
}

/// An error returned by [`RefCell::try_replace`](struct.RefCell.html#method.try_replace).
pub struct ReplaceError { _private: () }

/// An error returned by [`RefCell::try_swap`](struct.RefCell.html#method.try_swap).
pub struct SwapError { _private: () }

impl<T> RefCellTryReplaceExt<T> for RefCell<T> {
    /// Replaces the wrapped value with a new one, returning the old value,
    /// without deinitializing either one, or an error if the value is currently
    /// borrowed.
    ///
    /// This function corresponds to [`std::mem::replace`](../mem/fn.replace.html).
    ///
    /// This is the non-panicking variant of [`replace`](#method.replace)
    #[inline]
    fn try_replace(&self, t: T) -> Result<T, ReplaceError> {
        match self.try_borrow_mut() {
            Ok(mut b) => Ok(mem::replace(&mut *b, t)),
            Err(_) => Err(ReplaceError { _private: () })
        }
    }


    /// Swaps the wrapped value of `self` with the wrapped value of `other`,
    /// without deinitializing either one. Returns an error if either value is
    /// currently borrowed.
    ///
    /// This function corresponds to [`std::mem::swap`](../mem/fn.swap.html).
    ///
    /// This is the non-panicking variant of [`swap`](#method.swap)
    #[inline]
    fn try_swap(&self, other: &Self) -> Result<(), SwapError> {
        match (self.try_borrow_mut(), other.try_borrow_mut()) {
            (Ok(mut s), Ok(mut o)) => {
                mem::swap(&mut *s, &mut *o);
                Ok(())
            },
            _ => Err(SwapError { _private: () })
        }
    }
}
