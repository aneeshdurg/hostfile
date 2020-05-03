use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::{AddrParseError, IpAddr};
use std::path::Path;
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
#[derive(Debug, PartialEq)]
pub struct HostEntry {
    pub ip: IpAddr,
    pub names: Vec<String>,
}

impl FromStr for HostEntry {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut input = s;
        input = input.trim_start();

        let ip = parse_ip(input);
        if let Err(_) = ip {
            return Err("Couldn't parse a valid IP address");
        }
        let ip = ip.unwrap();
        input = ip.1;
        let ip = ip.0;

        match input.chars().next() {
            Some(' ') => {},
            _ => {
                return Err("Expected whitespace after IP");
            }
        }
        input = input.trim_start();

        let mut names = Vec::new();
        for name in input.split_whitespace() {
            // Account for comments at the end of the line
            match name.chars().next() {
                Some('#') => break,
                Some(_) => {},
                None => unreachable!(),
            }
            names.push(name.to_string());
        }

        Ok(HostEntry { ip, names })
    }
}

/// Parse a file using the format described in `man hosts(7)`
pub fn parse_file(path: &Path) -> Result<Vec<HostEntry>, &'static str> {
    if !path.exists() || !path.is_file() {
        return Err("File does not exist or is not a regular file");
    }

    let file = File::open(path);
    if file.is_err() {
        return Err("Could not open file");
    }
    let file = file.unwrap();

    let mut entries = Vec::new();

    let lines = BufReader::new(file).lines();
    for line in lines {
        if let Err(_) = line {
            return Err("Error reading file");
        }

        let line = line.unwrap();
        let line = line.trim_start();
        match line.chars().next() {
            // comment
            Some('#') => continue,
            // empty line
            None => continue,
            // valid line
            Some(_) => {},
        };

        entries.push(line.parse()?);
    }

    Ok(entries)
}

/// Parse /etc/hosts
pub fn parse_hostfile() -> Result<Vec<HostEntry>, &'static str> {
    parse_file(&Path::new("/etc/hosts"))
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

            # Comments can really be anywhere\n\
            bad:dad::ded    multiple hostnames for address\n\
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
            ))
        );
    }

    #[test]
    fn test_parse_file_errors() {
        let temp_file = Temp::new_file().unwrap();
        let temp_path = temp_file.as_path();
        let mut file = File::create(temp_path).unwrap();

        write!(file, "127.0.0.1localhost\n").expect("");
        assert_eq!(parse_file(&temp_path), Err("Expected whitespace after IP"));

        file.set_len(0).expect("Could not truncate file");
        file.seek(SeekFrom::Start(0)).expect("");
        write!(file, "127.0.0 localhost\n").expect("");
        assert_eq!(
            parse_file(&temp_path),
            Err("Couldn't parse a valid IP address")
        );

        file.set_len(0).expect("");
        file.seek(SeekFrom::Start(0)).expect("");
        write!(file, "127.0.0 local\nhost\n").expect("");
        assert_eq!(
            parse_file(&temp_path),
            Err("Couldn't parse a valid IP address")
        );

        let temp_dir = Temp::new_dir().unwrap();
        let temp_dir_path = temp_dir.as_path();
        assert_eq!(
            parse_file(&temp_dir_path),
            Err("File does not exist or is not a regular file")
        );
    }
}
