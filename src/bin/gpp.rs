use std::fs::File;
use std::io::{self, BufWriter};

use clap::{App, Arg};

fn main() -> Result<(), gpp::Error> {
    let matches = App::new("gpp")
        .version("0.6.2")
        .about("A Generic PreProcessor.")
        .author("Kestrer")
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
    let mut context = gpp::Context::new().exec(matches.is_present("allow_exec"));

    let (mut output_file, stdout, mut stdout_lock);
    let output: &mut dyn io::Write = if let Some(filename) = matches.value_of("output") {
        output_file = BufWriter::new(File::create(filename)?);
        &mut output_file
    } else {
        stdout = io::stdout();
        stdout_lock = stdout.lock();
        &mut stdout_lock
    };

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    for file in files {
        let data = if file == "-" {
            gpp::process_buf(&mut stdin, "<stdin>", &mut context)
        } else if let Some(text) = file.strip_prefix(':') {
            gpp::process_str(text, &mut context)
        } else {
            gpp::process_file(file, &mut context)
        }?;
        output.write_all(data.as_bytes())?;
    }
    Ok(())
}
