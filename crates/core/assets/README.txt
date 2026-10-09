RustBox guest tools disc
========================
1. Install the drivers: open the virtio-win disc and run virtio-win-guest-tools.exe
   (installs VirtIO drivers, the QXL display driver and the SPICE agent).
   Restart Windows, then in RustBox: Settings > Display > "QXL + SPICE".
2. Optional: right-click optimize-windows.cmd > Run as administrator.
   Turns off transparency/animations and background disk services so Windows
   feels smoother without a GPU.
