@echo off

setlocal EnableDelayedExpansion

title ImDisk Virtual Disk Driver Uninstall

echo ImDisk Virtual Disk Driver Uninstall
echo.

set IMDISK_TOTAL_DEVICES=0
set IMDISK_VALID_DEVICES=0
set IMDISK_PENDING_REMOVAL_DEVICES=0

if exist "%SystemRoot%\system32\imdisk.exe" (
  for /f %%a in ('imdisk -l -n') do call :addline "%%a"
)

if %IMDISK_VALID_DEVICES% GTR 0 (
  echo Number of existing ImDisk virtual disks: %IMDISK_VALID_DEVICES%
  echo.
  echo Please dismount all existing ImDisk virtual disks before uninstall!
  echo.
  if not "%IMDISK_SILENT_SETUP%" == "1" (
    pause
    start "" "%SystemRoot%\system32\control.exe" "%SystemRoot%\system32\imdisk.cpl"
  )
  endlocal
  goto :eof
)

echo Number of existing ImDisk virtual disks: %IMDISK_TOTAL_DEVICES%
echo.

set SYSDIR=%SystemRoot%\System32
set DRVDIR=%SystemRoot%\System32\drivers
set INFDIR=%SystemRoot%\inf

:: Stop services
echo Stopping services...
if %IMDISK_TOTAL_DEVICES% == 0 (
  "%SystemRoot%\system32\net.exe" stop imdsksvc >nul 2>&1
  "%SystemRoot%\system32\net.exe" stop awealloc >nul 2>&1
  "%SystemRoot%\system32\net.exe" stop imdisk >nul 2>&1
  "%SystemRoot%\system32\net.exe" stop DevIoDrv >nul 2>&1
  if exist "%SystemRoot%\system32\taskkill.exe" "%SystemRoot%\system32\taskkill.exe" /F /IM imdsksvc.exe >nul 2>&1
)

:: Delete services
echo Deleting services...
sc delete ImDskSvc >nul 2>&1
sc delete ImDisk >nul 2>&1
sc delete AWEAlloc >nul 2>&1
sc delete DevIoDrv >nul 2>&1

:: Delete driver files
echo Deleting driver files...
del /f /q "%DRVDIR%\imdisk.sys" >nul 2>&1
del /f /q "%DRVDIR%\awealloc.sys" >nul 2>&1
del /f /q "%DRVDIR%\deviodrv.sys" >nul 2>&1

:: Delete user-mode files
echo Deleting user-mode files...
del /f /q "%SYSDIR%\imdisk.exe" >nul 2>&1
del /f /q "%SYSDIR%\imdisk.cpl" >nul 2>&1
del /f /q "%SYSDIR%\imdsksvc.exe" >nul 2>&1

:: Delete INF and PNF files
del /f /q "%INFDIR%\imdisk.inf" >nul 2>&1
del /f /q "%INFDIR%\imdisk.pnf" >nul 2>&1

:: Delete registry entries
echo Removing registry entries...
reg delete "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /f >nul 2>&1
reg delete "HKCR\*\shell\ImDiskMountFile" /f >nul 2>&1
reg delete "HKCR\*\shell\ImDiskMountFileWriteable" /f >nul 2>&1
reg delete "HKCR\Drive\shell\ImDiskUnmount" /f >nul 2>&1
reg delete "HKCR\Drive\shell\ImDiskSaveImage" /f >nul 2>&1

echo.
echo ImDisk Virtual Disk Driver uninstalled.
echo A reboot may be required to complete the removal.
echo.

:: Delete ourselves last (schedule for deletion on reboot if needed)
del /f /q "%SYSDIR%\uninstall_imdisk.cmd" >nul 2>&1

endlocal
goto :eof

:addline

"%SystemRoot%\system32\imdisk.exe" -l -u %~1 > nul 2>&1

if errorlevel 1 (
  set /a IMDISK_PENDING_REMOVAL_DEVICES=%IMDISK_PENDING_REMOVAL_DEVICES% + 1 > nul
) else (
  set /a IMDISK_VALID_DEVICES=%IMDISK_VALID_DEVICES% + 1 > nul
)

set /a IMDISK_TOTAL_DEVICES=%IMDISK_TOTAL_DEVICES% + 1 > nul

goto :eof
