// Code taken from <https://github.com/WLBF/single-instance>
/*
MIT License

Copyright (c) 2018 LBF

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
 */

use nix::Result;
use nix::sys::socket::{self, UnixAddr};
use std::os::fd::{AsRawFd, OwnedFd};

/// A struct representing one running instance.
#[derive(Debug)]
pub struct SingleInstance {
    maybe_sock: Option<OwnedFd>,
}

impl SingleInstance {
    /// Returns a new SingleInstance object.
    pub fn new(name: &str) -> Result<Self> {
        let addr = UnixAddr::new_abstract(name.as_bytes())?;
        let sock = socket::socket(
            socket::AddressFamily::Unix,
            socket::SockType::Stream,
            socket::SockFlag::empty(),
            None,
        )?;

        let maybe_sock = match socket::bind(sock.as_raw_fd(), &addr) {
            Ok(()) => Some(sock),
            Err(nix::errno::Errno::EADDRINUSE) => None,
            Err(e) => return Err(e.into()),
        };

        Ok(Self { maybe_sock })
    }

    /// Returns whether this instance is single.
    pub fn is_single(&self) -> bool {
        self.maybe_sock.is_some()
    }
}

// impl Drop for SingleInstance {
//     fn drop(&mut self) {
//         if let Some(sock) = self.maybe_sock.as_ref() {
//             // Intentionally discard any close errors.
//             let _ = unistd::close(sock);
//         }
//     }
// }