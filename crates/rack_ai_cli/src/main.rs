use rack_ai_infrastructure::BootstrapMessage;

fn main() {
    let message = BootstrapMessage;
    println!("{}", message.value());
}
