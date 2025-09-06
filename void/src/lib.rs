pub use std::convert::Infallible as Void;

pub fn unreachable(_: Void) -> ! {
  unreachable!()
}
