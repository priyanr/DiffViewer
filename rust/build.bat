@echo off
setlocal
if defined LLVM_MINGW set "PATH=%LLVM_MINGW%\bin;%PATH%"
if exist "F:\priyan\tools\llvm-mingw\bin" set "PATH=F:\priyan\tools\llvm-mingw\bin;%PATH%"
if exist "%USERPROFILE%\.cargo\bin" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0"

cargo build --release || exit /b 1

if not exist build mkdir build
copy /y target\x86_64-pc-windows-gnullvm\release\diffviewer.exe build\DiffViewer.exe >nul
echo Build succeeded: rust\build\DiffViewer.exe
