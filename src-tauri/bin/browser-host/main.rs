fn main() {
    if localbridge_lib::browser_connection::host::run().is_err() {
        // Stdout is exclusively the binary native-messaging channel.
        eprintln!(
            "LocalBridge browser host closed: invalid origin, protocol, or local installation."
        );
        std::process::exit(1);
    }
}
