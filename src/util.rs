#[cfg(debug_assertions)]
#[macro_export]
macro_rules! debugprint {
    ($x: expr) => (eprintln!("{}", $x));
    ($x: expr, $($y: expr)+) => (eprint!("{} ", $x); debugprint!($($y),+));
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! debugprint {
    ($x: expr) => ();
    ($x: expr, $($y: expr)+) => ();
}