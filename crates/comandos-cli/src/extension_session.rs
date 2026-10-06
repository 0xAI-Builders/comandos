//! Native alias and explicit command share the same executor.
pub fn main(args: &[String]) -> i32 {
    comandos_runtime::extension_launch::executor::main(args)
}
