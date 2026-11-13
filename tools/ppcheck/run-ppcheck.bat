@echo off
rem ProgPoW GPU self-test for epic.iniaku.my.id. Stop the miner first.
rem Checks one GPU on four ProgPoW periods with three kernel variants (about 15 minutes).
rem Other GPU: run-ppcheck.bat 1  (GPU numbers start at 0)
cd /d "%~dp0"
set DEV=%1
if "%DEV%"=="" set DEV=0
set H=3796500 3796550 3796600 3796650
echo GPU %DEV% - results go to ppcheck-result.txt
echo [1/3] normal kernel
set INIAKU_PP_BARRIER2=
set INIAKU_PP_CLOPTS=
ppcheck.exe --device %DEV% --seconds 60 %H% > ppcheck-result.txt 2>&1
echo [2/3] extra barrier
set INIAKU_PP_BARRIER2=1
ppcheck.exe --device %DEV% --seconds 60 %H% >> ppcheck-result.txt 2>&1
echo [3/3] compiler optimizations off
set INIAKU_PP_BARRIER2=
set INIAKU_PP_CLOPTS=-cl-opt-disable
ppcheck.exe --device %DEV% --seconds 60 %H% >> ppcheck-result.txt 2>&1
type ppcheck-result.txt
echo.
echo Done. Please send ppcheck-result.txt.
pause
