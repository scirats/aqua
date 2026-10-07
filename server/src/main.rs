fn main() {
    if let Err(error) = aqua_server::run() {
        eprintln!("aqua-server: {error}");
        std::process::exit(1);
    }
}
