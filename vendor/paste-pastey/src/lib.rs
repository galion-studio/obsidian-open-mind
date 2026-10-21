// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `paste` 1.x API backed by `pastey` (galion deny-paste-replace, RUSTSEC-2024-0436).
//!
//! The archived `paste` crate exports three function-like proc macros: `paste!`, and the
//! hidden legacy `item!` and `expr!`. `pastey` keeps all three with the same syntax and
//! modifiers (`:lower`, `:upper`, `:snake`, `:camel`), so re-exporting them keeps every
//! `paste::paste!` / `use paste::paste;` call site in gemm and pulp working unchanged.

#![no_std]

pub use pastey::paste;

#[doc(hidden)]
pub use pastey::{expr, item};

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    // The same expansions gemm-common and pulp rely on: identifier concatenation and
    // case modifiers inside `macro_rules!`, plus item generation at module level.
    macro_rules! make_fn {
        ($name:ident, $ty:ident) => {
            crate::paste! {
                fn [<$name _ $ty>]() -> &'static str { stringify!([<$name _ $ty>]) }
                fn [<$name:upper _ $ty:camel>]() -> &'static str { stringify!([<$name:upper _ $ty:camel>]) }
            }
        };
    }
    make_fn!(gemm, f32);

    #[test]
    fn concat_and_case_modifiers_match_paste_1_0_15() {
        assert_eq!(gemm_f32(), "gemm_f32");
        assert_eq!(GEMM_F32(), "GEMM_F32");
        assert_eq!(crate::paste!(stringify!([<f _ 64>])), "f_64");
        assert_eq!(crate::paste!(stringify!([<"a" b 1>])), "ab1");
        assert_eq!(crate::paste!(stringify!([<MyType:snake>])), "my_type");
        assert_eq!(crate::paste!(stringify!([<my_type:camel>])), "MyType");
        assert_eq!(crate::paste!(stringify!([<abc:upper>])), "ABC");
        assert_eq!(crate::paste!(stringify!([<ABC:lower>])), "abc");
    }
}
