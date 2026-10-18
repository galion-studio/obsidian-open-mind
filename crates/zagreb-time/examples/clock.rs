//! Print the current Europe/Zagreb time and a fixed instant.
//!
//! Run: `nice -n 19 cargo run -j 2 -p zagreb-time --example clock`

fn main() {
    // A fixed winter instant (2026-12-01T12:00:00Z) so the output is stable.
    let winter = 1_796_126_400_i64;

    println!("now:   {}", zagreb_time::now_rfc3339());
    println!("winter: {}", zagreb_time::from_unix(winter));
    println!("label:  {}", zagreb_time::label(winter));
}
