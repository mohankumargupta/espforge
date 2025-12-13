1. Currently, only ESP32C3 is well supported, need the others as well.
2. need to check if I do

   ```sh
   espforge examples
   ```

   Then in the generated folder, if I make changes, then do

   ```sh
   espforge compile blink.yaml
   ```

  It should behave as expected.
3. We are currently lacking discoverability for yaml configuration and variables in ruchy scripting. Some ideas:
   - for yaml, have a menuconfig like thing, so I know what options are available
   - for ruchy, might need a subcommand that reads the yaml file and discovers what variables,
     components, devices, and global objects and methods are available.



