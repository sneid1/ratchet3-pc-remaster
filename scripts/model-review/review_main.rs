fn main() {
    uya_model_review::lower_priority();
    if let Err(e) = uya_model_review::material_review::serve() {
        eprintln!("review: {e}");
        std::process::exit(1);
    }
}
