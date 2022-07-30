use clap::Parser;

pub fn clap_parse_from<T: Parser>(args: Vec<String>) -> Result<T, clap::error::Error> {
    let mut clap_args = vec!["dummy".to_string()];
    clap_args.extend(args);
    T::try_parse_from(clap_args)
}
