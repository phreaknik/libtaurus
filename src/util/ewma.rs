use num::Float;
use num::One;
use std::ops::{Add, Mul, Sub};

/// Exponentially weighted moving average filter
pub struct EWMA<T>
where
    T: Add<Output = T> + Copy + Mul<Output = T> + Sub<Output = T> + One<Output = T> + Float,
{
    pub curr: T,
    pub alpha: T,
}

impl<T> EWMA<T>
where
    T: Add<Output = T> + Copy + Mul<Output = T> + Sub<Output = T> + One<Output = T> + Float,
{
    /// Instantiate a new EWMA filter
    pub fn new(initial: T, alpha: T) -> EWMA<T> {
        EWMA {
            curr: initial,
            alpha,
        }
    }

    /// Insert a new sample into the EWMA filter, and return the new current value of the filter
    pub fn insert(&mut self, val: T) -> T {
        let one: T = One::one();
        self.curr = (self.alpha * val) + (one - self.alpha) * self.curr;
        self.curr
    }
}
