use std::io::{self, BufReader, Write};
use clap::{App, Arg};

fn main() -> Result<(), gpp::Error> {
    let matches = App::new("gpp")
        .version("0.5.0")
        .about("A Generic PreProcessor.")
        .author("Koxiaet")
        .arg(Arg::with_name("allow_exec")
            .help("Whether #exec and #in commands are allowed")
            .short("-e")
            .long("--allow-exec")
        )
        .arg(Arg::with_name("files")
            .help("The files to preprocess. - means stdin, and any 'filename' starting with a colon is treated as a literal string to preprocess. If no files are given, it will default to stdin.")
            .default_value("-")
            .multiple(true)
        )
        .get_matches();
    
    let files = matches.values_of("files").unwrap();
    let mut context = if matches.is_present("allow_exec") {
        gpp::Context::new_exec()
    } else {
        gpp::Context::new()
    };

    for file in files {
        io::stdout().write(if file == "-" {
            gpp::process_buf(BufReader::new(io::stdin()), "<stdin>", &mut context)
        } else if file.starts_with(":") {
            gpp::process_str(&file[1..], &mut context)
        } else {
            gpp::process_file(file, &mut context)
        }?.as_bytes())?;
    }
    Ok(())
}
