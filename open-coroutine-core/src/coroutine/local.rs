use dashmap::DashMap;
use std::ffi::c_void;

/// A struct for coroutines handles local args.
#[derive(Debug, Default)]
pub struct CoroutineLocal<'c>(DashMap<&'c str, usize>);

#[allow(missing_docs, box_pointers)]
impl<'c> CoroutineLocal<'c> {
    pub fn put<V>(&self, key: &'c str, val: V) -> Option<V> {
        let v = Box::leak(Box::new(val));
        self.0
            .insert(key, v as *mut V as usize)
            .map(|ptr| unsafe { *Box::from_raw((ptr as *mut c_void).cast::<V>()) })
    }

    #[must_use]
    pub fn get<V>(&self, key: &'c str) -> Option<&V> {
        self.0
            .get(key)
            .map(|ptr| unsafe { &*(*ptr as *mut c_void).cast::<V>() })
    }

    #[must_use]
    pub fn get_mut<V>(&self, key: &'c str) -> Option<&mut V> {
        self.0
            .get(key)
            .map(|ptr| unsafe { &mut *(*ptr as *mut c_void).cast::<V>() })
    }

    #[must_use]
    pub fn remove<V>(&self, key: &'c str) -> Option<V> {
        self.0
            .remove(key)
            .map(|ptr| unsafe { *Box::from_raw((ptr.1 as *mut c_void).cast::<V>()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local() {
        let local = CoroutineLocal::default();
        assert!(local.put("1", 1).is_none());
        assert_eq!(Some(1), local.put("1", 2));
        assert_eq!(2, *local.get("1").unwrap());
        *local.get_mut("1").unwrap() = 3;
        assert_eq!(Some(3), local.remove("1"));
    }
}
