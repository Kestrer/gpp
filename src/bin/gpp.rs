use clap::{App, Arg};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};

fn main() -> Result<(), gpp::Error> {
    let matches = App::new("gpp")
        .version("0.5.1")
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
        .arg(Arg::with_name("output")
            .help("The output file. Defaults to stdout.")
            .short("-o")
            .long("--output")
            .takes_value(true)
        )
        .get_matches();

    let files = matches.values_of("files").unwrap();
    let mut context = if matches.is_present("allow_exec") {
        gpp::Context::new_exec()
    } else {
        gpp::Context::new()
    };
    let mut output = match matches.value_of("output") {
        Some(filename) => Some(BufWriter::new(File::create(filename)?)),
        None => None,
    };

    for file in files {
        let data = if file == "-" {
            gpp::process_buf(BufReader::new(io::stdin()), "<stdin>", &mut context)
        } else if file.starts_with(":") {
            gpp::process_str(&file[1..], &mut context)
        } else {
            gpp::process_file(file, &mut context)
        }?;
        let bytes = data.as_bytes();
        match &mut output {
            Some(file) => file.write(bytes),
            None => io::stdout().write(bytes),
        }?;
    }
    Ok(())
}
