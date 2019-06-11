Config Management
=================
- Railgun does not use config files.
- Only env vars are used for config. This is much more scalable and transferable. If a file is needed, the `.env` file paradigm may be used as needed.
- Config is idempotent, and many config options will only ever be used/evaluated when a node is coming online for the first time.
- Nodes should only ever need to be restarted for upgrading the version of Railgun or for major config changes.
