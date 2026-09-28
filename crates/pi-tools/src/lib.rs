//! 内置工具：bash、read、write、edit、grep、find、ls 等的参数校验与执行。

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "pi-tools");
    }
}
