use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::{AddrParseError, IpAddr};
use std::path::{PathBuf, Path};
use std::str::FromStr;

/**
 * Host file format:
 *   File:
 *     Line |
 *     Line newline File
 *
 *   Line:
 *     Comment | Entry
 *
 *   Comment:
 *     # .* newline
 *
 *   Entry:
 *     ws* ip ws+ Name (ws+ Names | $)
 *        (where ip is parsed according to std::net)
 *
 *   ws: space | tab
 *
 *   Name:
 *     [a-z.-]+
 *
 *   Names:
 *     Name ws* | Name ws+ Names
 */

fn parse_ip(input: &str) -> Result<(IpAddr, &str), AddrParseError> {
    let non_ip_char_idx = input.find(|c: char| c != '.' && c != ':' && !c.is_digit(16));
    let (ip, remainder) = input.split_at(non_ip_char_idx.unwrap_or(input.len()));
    Ok((ip.parse()?, remainder))
}

/// A struct representing a line from /etc/hosts that has a host on it
#[derive(Debug, Clone, PartialEq)]
pub struct HostEntry {
    pub ip: IpAddr,
    pub names: Vec<String>,
}

impl FromStr for HostEntry {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut input = s;
        input = input.trim_start();

        let ip = parse_ip(input);
        if let Err(err) = ip {
            return Err(format!("Couldn't parse a valid IP address: {err}"));
        }
        let ip = ip.unwrap();
        input = ip.1;
        let ip = ip.0;

        match input.chars().next() {
            Some(' ') | Some('\t') => {}
            _ => {
                return Err("Expected whitespace after IP".to_string());
            }
        }
        input = input.trim_start();

        let mut names = Vec::new();
        for name in input.split_whitespace() {
            // Account for comments at the end of the line
            match name.chars().next() {
                Some('#') => break,
                Some(_) => {}
                None => unreachable!(),
            }
            names.push(name.to_string());
        }

        Ok(HostEntry { ip, names })
    }
}

/// Parse a file using the format described in `man hosts(7)`
pub fn parse_file(path: &Path) -> Result<Vec<HostEntry>, String> {
    if !path.exists() || !path.is_file() {
        return Err(format!(
            "File ({:?}) does not exist or is not a regular file",
            path
        ));
    }

    let file = File::open(path);
    if file.is_err() {
        return Err(format!("Could not open file ({:?})", path));
    }
    let file = file.unwrap();

    let mut entries = Vec::new();

    let lines = BufReader::new(file).lines();
    let mut line_count = 1;
    for line in lines {
        if let Err(err) = line {
            return Err(format!("Error reading file at line {line_count}: {err}"));
        }

        let line = line.unwrap();
        let line = line.trim_start();
        match line.chars().next() {
            // comment
            Some('#') => continue,
            // empty line
            None => continue,
            // valid line
            Some(_) => {}
        };
        match line.parse() {
            Ok(parsed_host_entry) => entries.push(parsed_host_entry),
            Err(err) => {
                return Err(format!("{err} at line {line_count} with content: '{line}'"));
            }
        }
        line_count += 1;
    }

    Ok(entries)
}

/// Parse system hostfile.
///
/// - `/etc/hosts` on Unix.
/// - `C:\Windows\system32\drivers\etc\hosts` on Windows.
pub fn parse_hostfile() -> Result<Vec<HostEntry>, String> {
    parse_file(&get_hostfile_path()?)
}

/// Get path to the system hostfile.
pub fn get_hostfile_path() -> Result<PathBuf, String> {
    #[cfg(not(windows))]
    {
        Ok(PathBuf::from("/etc/hosts"))
    }

    #[cfg(windows)]
    {
        // Implementation adapted from cargo's `home`.
        // See https://crates.io/crates/home
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        use std::ptr::null_mut;
        use std::slice;
        use windows_sys::Win32::{
            Foundation::S_OK,
            System::Com::CoTaskMemFree,
            UI::Shell::{FOLDERID_System, SHGetKnownFolderPath, KF_FLAG_DONT_VERIFY},
        };

        extern "C" {
            fn wcslen(buf: *const u16) -> usize;
        }

        let mut ptr = null_mut::<u16>();
        let ret = unsafe {
            SHGetKnownFolderPath(
                &FOLDERID_System,
                KF_FLAG_DONT_VERIFY as u32,
                null_mut(),
                &mut ptr,
            )
        };

        match ret {
            S_OK => {
                let path_slice = unsafe { slice::from_raw_parts(ptr, wcslen(ptr)) };
                let os_str = OsString::from_wide(path_slice);
                unsafe { CoTaskMemFree(ptr.cast()) };
                let mut pathbuf = PathBuf::from(&os_str);
                pathbuf.push("drivers\\etc\\hosts");
                Ok(pathbuf)
            }
            _ => {
                // free any allocated memory even on failure (a null ptr is a no-op for `CoTaskMemFree`)
                unsafe { CoTaskMemFree(ptr.cast()) };
                Err(format!(
                    "Could not get path to Windows hosts file: {}",
                    std::io::Error::last_os_error(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate mktemp;
    use mktemp::Temp;

    use std::io::{Seek, SeekFrom, Write};
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn parse_ipv4() {
        let input = "127.0.0.1";
        assert_eq!(
            parse_ip(input),
            Ok((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), ""))
        );
    }

    #[test]
    fn parse_ipv6() {
        let input = "::1";
        assert_eq!(
            parse_ip(input),
            Ok((IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), ""))
        );
    }

    #[test]
    fn parse_entry() {
        assert_eq!(
            "127.0.0.1 localhost".parse(),
            Ok(HostEntry {
                ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                names: vec!(String::from("localhost")),
            })
        );
    }

    #[test]
    fn parse_entry_multiple_names() {
        assert_eq!(
            "127.0.0.1 localhost home  ".parse(),
            Ok(HostEntry {
                ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                names: vec!(String::from("localhost"), String::from("home")),
            })
        );
    }

    #[test]
    fn parse_entry_ipv6() {
        assert_eq!(
            "::1 localhost".parse(),
            Ok(HostEntry {
                ip: IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                names: vec!(String::from("localhost")),
            })
        );
    }

    #[test]
    fn parse_entry_with_ws_and_comments() {
        assert_eq!(
            "    ::1 \tlocalhost # comment".parse(),
            Ok(HostEntry {
                ip: IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                names: vec!(String::from("localhost")),
            })
        );
    }

    #[test]
    fn test_parse_file() {
        let temp_file = Temp::new_file().unwrap();
        let temp_path = temp_file.as_path();
        let mut file = File::create(temp_path).unwrap();

        write!(
            file,
            "\
            # This is a sample hosts file\n\
               \n# Sometimes hosts files can have wonky spacing
            127.0.0.1       localhost\n\
            ::1             localhost\n\
            255.255.255.255 broadcast\n\
            \n\
            # Comments can really be anywhere\n\
            bad:dad::ded    multiple hostnames for address\n\
            1.1.1.1\ttabSeperatedHostname\n\
            1.1.1.2\t tabAndSpaceSeparatedHostName\n\
            \t1.1.1.3\t\t\tlineStartsWithTab\n\
              1.1.1.4 lineStartsWithSpace\n\
            1.1.1.5 skip_blank_line
        "
        )
        .expect("Could not write to temp file");

        assert_eq!(
            parse_file(&temp_path),
            Ok(vec!(
                HostEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    names: vec!(String::from("localhost")),
                },
                HostEntry {
                    ip: IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                    names: vec!(String::from("localhost")),
                },
                HostEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                    names: vec!(String::from("broadcast")),
                },
                HostEntry {
                    ip: IpAddr::V6(Ipv6Addr::new(0xbad, 0xdad, 0, 0, 0, 0, 0, 0xded)),
                    names: vec!(
                        String::from("multiple"),
                        String::from("hostnames"),
                        String::from("for"),
                        String::from("address")
                    ),
                },
                HostEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
                    names: vec!(String::from("tabSeperatedHostname")),
                },
                HostEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 2)),
                    names: vec!(String::from("tabAndSpaceSeparatedHostName")),
                },
                HostEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 3)),
                    names: vec!(String::from("lineStartsWithTab")),
                },
                HostEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 4)),
                    names: vec!(String::from("lineStartsWithSpace")),
                },
                HostEntry {
                    ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 5)),
                    names: vec!(String::from("skip_blank_line")),
                },
            ))
        );
    }

    #[test]
    fn test_parse_file_errors() {
        let temp_file = Temp::new_file().unwrap();
        let temp_path = temp_file.as_path();
        let mut file = File::create(temp_path).unwrap();

        write!(file, "127.0.0.1localhost\n").expect("");
        assert_eq!(
            parse_file(&temp_path),
            Err(
                "Expected whitespace after IP at line 1 with content: '127.0.0.1localhost'"
                    .to_string()
            )
        );

        file.set_len(0).expect("Could not truncate file");
        file.seek(SeekFrom::Start(0)).expect("");
        write!(file, "127.0.0 localhost\n").expect("");
        assert_eq!(
            parse_file(&temp_path),
            Err("Couldn't parse a valid IP address: invalid IP address syntax at line 1 with content: '127.0.0 localhost'".to_string())
        );

        file.set_len(0).expect("");
        file.seek(SeekFrom::Start(0)).expect("");
        write!(file, "127.0.0 local\nhost\n").expect("");
        assert_eq!(
            parse_file(&temp_path),
            Err("Couldn't parse a valid IP address: invalid IP address syntax at line 1 with content: '127.0.0 local'".to_string())
        );

        file.set_len(0).expect("");
        file.seek(SeekFrom::Start(0)).expect("");
        write!(file, "127.0.0.1 localhost\nlocalhost myhost").expect("");
        assert_eq!(
            parse_file(&temp_path),
            Err("Couldn't parse a valid IP address: invalid IP address syntax at line 2 with content: 'localhost myhost'".to_string())
        );

        let temp_dir = Temp::new_dir().unwrap();
        let temp_dir_path = temp_dir.as_path();
        assert_eq!(
            parse_file(&temp_dir_path),
            Err(format!(
                "File ({:?}) does not exist or is not a regular file",
                temp_dir_path
            ))
        );
    }

    #[test]
    fn test_clone() {
        let host_entry = HostEntry {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 42, 42)),
            names: vec![String::from("comp1"), String::from("computer1")],
        };
        let cloned = host_entry.clone();
        assert_eq!(host_entry, cloned)
    }

    #[test]
    fn test_get_hostfile_path() {
        let maybe_path = get_hostfile_path();
        assert!(maybe_path.is_ok());
        assert!(maybe_path.unwrap().exists());
    }

    // The next test only runs on GitHub Actions.
    //
    // Unix systems *typically* include localhost in /etc/hosts,
    // but we only assume that for GitHub Actions hostfile.
    //
    // In GitHub Actions, the Windows runnner includes an entry like
    // "10.1.0.85 <long-whatever>.cloudapp.net", localhost is commented.
    #[test_with::env(GITHUB_ACTIONS)]
    #[test]
    fn test_parse_hostfile() {
        let maybe_hostfile = parse_hostfile();
        assert!(maybe_hostfile.is_ok());
        let hostfile = maybe_hostfile.unwrap();
        assert!(!hostfile.is_empty());

        #[cfg(not(windows))]
        {
            let localhost = hostfile
                .iter()
                .find(|entry| entry.names.contains(&String::from("localhost")));
            assert!(localhost.is_some());
        }
    }
}
