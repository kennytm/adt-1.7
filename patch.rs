extern mod std;

use std::getopts::Matches;
use std::tempfile::mkdtemp;
use core::result::{Ok, Err};
use core::option::{None, Some};

fn main() {
    let (input_jar, output_jar) = match fetch_matches() {
        None => return,
        Some(ref m) => get_paths(m)
    };

    let class_dir = extract_jar(&input_jar);
    let class_path = class_dir.push(k_class_name);
    match find_5_offset(&class_path) {
        Ok(offset) => replace_5_as_7(&class_path, offset),
        Err(msg) => { io::println(msg); return; }
    };

    update_jar(&class_dir, &input_jar, &output_jar);
    recursively_remove_file(&class_dir);

    io::println(fmt!(
        "Patch complete. You may now want to replace\n  %s\nby\n  %s\n",
        input_jar.to_str(), output_jar.to_str()));
}


const k_brief_usage: &str = "
Usage: ./patch [-i com.android.ide.eclipse.adt_xxxxx.jar] [-o new.adt.jar]

Patch the Eclipse ADT plugin to enable Java 7 compatibility (while disabling
Java 1.5).
";


const k_class_name: &str = "com/android/ide/eclipse/adt/AdtConstants.class";


/**
 * Fetch the command line arguments.
 */
fn fetch_matches() -> Option<Matches> {
    use std::getopts::groups::{optopt, optflag, getopts, usage};
    use std::getopts::{fail_str, opt_present};

    let options = [
        optopt("i", "", "\
            The input *.jar. If not provided, this program will search for one \
            in the filesystem in the default location.", "x.jar"),
        optopt("o", "", "\
            The output *.jar. If not provided, the output will be written to \
            the working directory.", "x.jar"),
        optflag("h", "", "Show this help text."),
    ];

    match getopts(os::args(), options) {
        Err(e) => {
            io::println(fail_str(e));
            None
        },
        Ok(matches) => {
            if opt_present(&matches, "h") {
                io::println(usage(k_brief_usage, options));
                None
            } else {
                Some(matches)
            }
        }
    }
}


/**
 * Get the paths of the input and output *.jar.
 */
fn get_paths(matches: &Matches) -> (path::Path, path::Path) {
    use std::getopts::opt_maybe_str;

    let input_jar = match opt_maybe_str(matches, "i") {
        Some(path) => path::Path(path),
        None => find_jar()
    };

    let output_jar = match opt_maybe_str(matches, "o") {
        Some(path) => path::Path(path),
        None => os::getcwd().push(get_default_move(input_jar.filename(),
                                                   ~"adt.jar"))
    };

    (input_jar, output_jar)
}


/**
 * Find the default input *.jar.
 */
fn find_jar() -> path::Path {
    let possible_paths = [
        path::Path("/usr/share/eclipse/dropins/android/eclipse/plugins/"),
    ];

    for possible_paths.each |path| {
        if !os::path_is_dir(path) {
            loop;
        }

        for os::list_dir_path(path).each |jar_path| {
            match jar_path.filename() {
                Some(n) =>
                    if n.starts_with("com.android.ide.eclipse.adt_")
                            && n.ends_with(".jar") {
                        return copy **jar_path;
                    },
                _ => loop
            }
        }
    }

    fail ~"Cannot find the ADT jar. Please use the '-i' flag.";
}


/**
 * If the option is not none, move it into the result. Otherwise, move the
 * default value into the result.
 */
fn get_default_move<T: Owned>(opt: Option<T>, def: T) -> T {
    match opt {
        Some(t) => t,
        None => def
    }
}


/**
 * Extract the input *.jar. Returns the path of the extracted location.
 */
fn extract_jar(input_jar: &path::Path) -> path::Path {
    let class_root = option::expect(mkdtemp(&os::tmpdir(), "-adt-jar"),
                                    "Cannot create temporary directory");
    jar([~"xf", input_jar.to_str(), k_class_name.to_str()], &class_root);
    return class_root;
}


/**
 * Perform the 'jar' command in a particular directory.
 */
fn jar(args: &[~str], class_root: &path::Path) {
    let jar_res = run::waitpid(run::spawn_process("jar", args, &None,
                                                  &Some(class_root.to_str()),
                                                  0, 0, 0));
    if jar_res != 0 {
        fail fmt!("Executing 'jar' failed, error #%d.", jar_res);
    }
}


/**
 * Find the offset of the '5' of the constant "1.5" in the *.class file.
 */
fn find_5_offset(class_path: &path::Path) -> result::Result<uint, ~str> {
    use io::ReaderUtil;

    let reader = io::file_reader(class_path).get();

    if reader.read_be_u32() != 0xcafebabe {
        return Err(~"Not a *.class file: Magic does not match.");
    }

    let skip = |count| {
        reader.seek(count, io::SeekCur)
    };

    skip(4);
    let pool_size = reader.read_be_u16() - 1;
    for pool_size.times || {
        match reader.read_byte() {
            1 => {
                let length = reader.read_be_u16();
                if length != 3 {
                    skip(length as int);
                } else {
                    let bytes = reader.read_bytes(length as uint);
                    if bytes == ~[0x31, 0x2e, 0x35] {
                        return Ok(reader.tell() - 1);
                    }
                }
            },
            3 | 4 | 9 | 10 | 11 | 12 => skip(4),
            5 | 6 => skip(8),
            7 | 8 => skip(2),
            _ => return Err(~"Not a *.class file: \
                              Constant pool ended prematurely \
                              or invalid constant type.")
        }
    }

    return Err(~"Cannot find the constant '1.5'.");
}


/**
 * Replace the '5' of the constant "1.5" by the character '7' in the *.class
 * file.
 */
fn replace_5_as_7(class_path: &path::Path, offset: uint) {
    use io::WriterUtil;

    do os::as_c_charp(class_path.to_str()) |raw_class_path| {
        do os::as_c_charp("r+") |raw_mode| {
            let file = libc::funcs::c95::stdio::fopen(raw_class_path, raw_mode);
            let writer = io::FILE_writer(file, true);
            writer.seek(offset as int, io::SeekSet);
            writer.write_u8(0x37);
        }
    }
}


/**
 * Update the output *.jar by replacing the interesting *.class file by our
 * patched one.
 */
fn update_jar(class_root: &path::Path,
              input_jar: &path::Path, output_jar: &path::Path) {
    os::copy_file(input_jar, output_jar);
    jar([~"uf", output_jar.to_str(), k_class_name.to_str()], class_root);
}


/**
 * Recursively remove all files under (inclusively) 'root'. This is similar to
 * the 'rm -r' command.
 */
fn recursively_remove_file(root: &path::Path) {
    if os::path_is_dir(root) {
        for os::list_dir_path(root).each |path| {
            recursively_remove_file(*path);
        }
        os::remove_dir(root);
    } else {
        os::remove_file(root);
    }
}

/*-- GPLv3 ---------------------------------------------------------------------

patch.rs - Patch ADT to enable Java 7.
Copyright (C) 2012  Kenny Chan <kennytm@gmail.com>

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE.  See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with
this program.  If not, see <http://www.gnu.org/licenses/>.

--- GPLv3 --------------------------------------------------------------------*/

