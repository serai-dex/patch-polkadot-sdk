pub use std::convert::Infallible as Void;

pub fn unreachable(x: Void) -> ! {
  unreachable!()
}
