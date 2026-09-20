/// A best-effort in-memory secret: stores a String and zeros its bytes on
/// Drop. Not a vault — the secret is already in process memory if passed via
/// env/arg — but it avoids leaving it around longer than necessary and uses
/// constant-time comparison to avoid trivial timing attacks.
pub struct Secret {
    inner: String,
}

impl Clone for Secret {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secret")
            .field("len", &self.inner.len())
            .finish()
    }
}

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self { inner: s.into() }
    }

    pub fn push(&mut self, c: char) {
        self.inner.push(c);
    }

    pub fn pop(&mut self) {
        self.inner.pop();
    }

    pub fn clear(&mut self) {
        // SAFETY: we are about to clear the String, so invalid UTF-8 bytes
        // briefly present in the capacity buffer are fine; clear() resets length.
        unsafe {
            self.inner.as_bytes_mut().fill(0);
        }
        self.inner.clear();
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Constant-time equality over bytes. Always returns false for unequal
    /// lengths to avoid leaking length via timing.
    pub fn constant_time_eq(&self, other: &Self) -> bool {
        let a = self.inner.as_bytes();
        let b = other.inner.as_bytes();
        if a.len() != b.len() {
            return false;
        }
        let mut acc = 0u8;
        for (x, y) in a.iter().zip(b.iter()) {
            acc |= x ^ y;
        }
        acc == 0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_and_rejects() {
        let a = Secret::new("hunter2");
        let b = Secret::new("hunter2");
        let c = Secret::new("hunter3");
        let d = Secret::new("hunter22");
        assert!(a.constant_time_eq(&b));
        assert!(!a.constant_time_eq(&c));
        assert!(!a.constant_time_eq(&d));
    }

    #[test]
    fn clear_resets() {
        let mut s = Secret::new("abc");
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn push_and_pop() {
        let mut s = Secret::new("");
        s.push('x');
        s.push('y');
        assert!(!s.is_empty());
        s.pop();
        let two = Secret::new("x");
        assert!(s.constant_time_eq(&two));
    }
}
