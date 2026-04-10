//! Declarative macros for nested [`Then`](crate::Then) (arrow `>>>`) without deep right-nesting.

/// Right-associative pipeline: `scan_then!(a, b, c)` → `Then { left: a, right: Then { left: b, right: c } }`.
#[macro_export]
macro_rules! scan_then {
    ($head:expr $(, $tail:expr)+ $(,)?) => {
        $crate::__scan_then_impl!($head $(, $tail)+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __scan_then_impl {
    ($a:expr, $b:expr) => {
        $crate::Then {
            left: $a,
            right: $b,
        }
    };
    ($a:expr, $b:expr, $($rest:expr),+) => {
        $crate::Then {
            left: $a,
            right: $crate::__scan_then_impl!($b $(, $rest)+),
        }
    };
}

/// `split!(a, b)` → [`Split`](crate::Split) `{ left: a, right: b }`.
#[macro_export]
macro_rules! scan_split {
    ($left:expr, $right:expr $(,)?) => {
        $crate::Split {
            left: $left,
            right: $right,
        }
    };
}

/// `merge!(a, b)` → [`Merge`](crate::Merge).
#[macro_export]
macro_rules! scan_merge {
    ($left:expr, $right:expr $(,)?) => {
        $crate::Merge {
            left: $left,
            right: $right,
        }
    };
}

/// `choose!(a, b)` → [`Choose`](crate::Choose).
#[macro_export]
macro_rules! scan_choose {
    ($left:expr, $right:expr $(,)?) => {
        $crate::Choose {
            left: $left,
            right: $right,
        }
    };
}

/// `fanin!(a, b)` → [`Fanin`](crate::Fanin).
#[macro_export]
macro_rules! scan_fanin {
    ($left:expr, $right:expr $(,)?) => {
        $crate::Fanin {
            left: $left,
            right: $right,
        }
    };
}

/// `both!(a, b)` → [`ZipTuple`](crate::ZipTuple) / [`Both`](crate::Both).
#[macro_export]
macro_rules! scan_both {
    ($left:expr, $right:expr $(,)?) => {
        $crate::ZipTuple {
            left: $left,
            right: $right,
        }
    };
}
