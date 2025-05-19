nobscount
=========

> no bullshit visitor counter

Counts visitors on your webpage. No analytics, no JS, just count.
Highly configurable; can count unique visits, filter visitors based on IP and 
user-agent blacklists.

Default settings should be fine for most users; digits are stored in `img` 
directory, and `example.html` showcases a basic counter. You might want to apply
recommended user-agent filter in `config.toml` to exclude crawlers from the 
count.


Usage
-----

1. Install Rust: <https://www.rust-lang.org/tools/install>

2. Build the project:

```
cargo build --release
```

You can also install musl toolchain 
(`rustup target add x86_64-unknown-linux-musl`) and add 
`--target x86_64-unknown-linux-musl` to a build command to avoid glibc 
conflicts.

3. Copy an executable `nobscount` from `target/release` or 
   `target/x86_64-unknown-linux-musl/release` somewhere.

4. Run it:

```
./nobscount >> log.txt 2>&1 &
```

5. (optional) Add rerouting to your webserver config. I personally bind the 
   counter to an internal address `172.18.0.1:1234` and pass it through in nginx
   like so:

```config
# Counter
    location /counter/increment {
        proxy_set_header        X-Real-IP       $remote_addr;
        proxy_set_header        X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_pass http://172.18.0.1:1234/increment;
    }

    location /counter/get {
        proxy_set_header        X-Real-IP       $remote_addr;
        proxy_set_header        X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_pass http://172.18.0.1:1234/get;
    }
```

`X-Real-IP` allows counter to get an actual visitor IP and not 
the IP of a server. I then use `/counter/increment` and `/counter/get` in my 
HTML.


Configuring
-----------

Configuration is done using `config.toml` file. The settings are 
self-explainatory; if you wish to restart the counter after changing the config,
you can run:

```
yes | ./nobscount >> log.txt 2>&1 &
```

This will automatically stop the old instance and launch the new one.


Contact
-------

Contact me if you need any help: <visilii@disroot.org>


License
-------

See LICENSE.