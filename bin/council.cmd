@echo off
rem council: the Council Loop command, from a checkout or a plugin folder (Windows).
rem Runs the Rust binary when there is one: named by COUNCIL_BIN, built in this checkout, or on PATH.
rem Otherwise the Python reference, which takes the same commands and prints the same lines.
setlocal
set "HERE=%~dp0.."
if not defined COUNCIL_SKILL_DIR set "COUNCIL_SKILL_DIR=%HERE%\skills\council-loop"
set "BIN="
if defined COUNCIL_BIN if exist "%COUNCIL_BIN%" set "BIN=%COUNCIL_BIN%"
if not defined BIN if not defined COUNCIL_PYTHON if exist "%HERE%\target\release\council.exe" set "BIN=%HERE%\target\release\council.exe"
if not defined BIN if not defined COUNCIL_PYTHON for %%I in (council.exe) do set "BIN=%%~$PATH:I"
if defined BIN goto binary

set "PY=python"
if defined COUNCIL_PYTHON set "PY=%COUNCIL_PYTHON%"
set "SCRIPT=%HERE%\skills\council-loop\scripts\council.py"
if /i "%~1"=="mcp" goto server
if /i "%~1"=="hook" goto hook
"%PY%" "%SCRIPT%" %*
exit /b %ERRORLEVEL%

:server
"%PY%" "%HERE%\mcp\server.py"
exit /b %ERRORLEVEL%

:hook
"%PY%" "%HERE%\hooks\protect_locked_files.py"
exit /b %ERRORLEVEL%

:binary
"%BIN%" %*
exit /b %ERRORLEVEL%
