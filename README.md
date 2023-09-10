# heffalump conduit and installer
## hotsync conduit for Heffalump (palmOS mastadon client)


### installation
this crate exports two build targets:
1. the `heffalump_conduit` library, which produces `heffalump_conduit.dll`
2. the `heffalump_conduit_install` executable, which produces `heffalump_conduit_install.exe`


to install the conduit:
1. replace the environment variables in `.cargo/config` with the relevant tokens
2. build both targets with `cargo build --release`
3. copy `heffalump_conduit.dll` to the same folder as the hotsync executable
4. run `heffalump_conduit_install.exe`


