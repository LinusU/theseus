# EXE parser

This crate parses executable file formats:

- DOS .exe files,
- Windows Portable Executable (PE) files, including .exe, .dll, and .icd images.

It doesn't interact with any of the emulation machinery, it just accepts byte
buffers and returns different views on to them.
