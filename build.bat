@echo off
setlocal
rem llvm-mingw from LLVM_MINGW when it is set, otherwise the clang already on PATH.
if defined LLVM_MINGW set "PATH=%LLVM_MINGW%\bin;%PATH%"
cd /d "%~dp0"
if not exist build mkdir build

rem No C runtime: own entry point, only system DLLs. -Werror keeps the build warning-free.
set CFLAGS=-std=c11 -O2 -Wall -Wextra -Werror -Wno-missing-field-initializers ^
 -DUNICODE -D_UNICODE -DWIN32_LEAN_AND_MEAN -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00 ^
 -ffunction-sections -fdata-sections
set LDFLAGS=-nostdlib -Wl,-e,entry -Wl,--subsystem,windows -Wl,--gc-sections -s ^
 -lkernel32 -luser32 -lgdi32 -ladvapi32 -ldwmapi -luxtheme

x86_64-w64-mingw32-windres --no-preprocess app.rc -O coff -o build\app.res || exit /b 1
x86_64-w64-mingw32-clang %CFLAGS% src\main.c src\git.c src\syntax.c build\app.res -o build\DiffViewer.exe %LDFLAGS% || exit /b 1
