@echo off
setlocal EnableDelayedExpansion

:: Check for admin privileges
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo This script requires administrator privileges.
    echo Please run as administrator.
    pause
    exit /b 1
)

:: Detect architecture (can be passed as parameter from config.exe)
set ARCH=
if /i "%~1"=="ARM64" set ARCH=ARM64
if /i "%~1"=="amd64" set ARCH=amd64
if "%ARCH%"=="" if "%PROCESSOR_ARCHITECTURE%"=="ARM64" set ARCH=ARM64
if "%ARCH%"=="" if "%PROCESSOR_ARCHITECTURE%"=="AMD64" set ARCH=amd64
if "%ARCH%"=="" (
    echo Unsupported architecture: %PROCESSOR_ARCHITECTURE%
    exit /b 1
)

echo Installing ImDisk Virtual Disk Driver for %ARCH%...

:: Get script directory
set SRCDIR=%~dp0
set SRCDIR=%SRCDIR:~0,-1%

:: Destination directories
set SYSDIR=%SystemRoot%\System32
set DRVDIR=%SystemRoot%\System32\drivers
set INFDIR=%SystemRoot%\inf

:: Stop services if running
echo Stopping existing services...
net stop ImDskSvc >nul 2>&1
net stop ImDisk >nul 2>&1
net stop AWEAlloc >nul 2>&1
net stop DevIoDrv >nul 2>&1

:: Copy driver files
echo Copying driver files...
copy /y "%SRCDIR%\sys\%ARCH%\ImDisk.sys" "%DRVDIR%\imdisk.sys" >nul
if %errorlevel% neq 0 (echo Failed to copy imdisk.sys & exit /b 1)

copy /y "%SRCDIR%\awealloc\%ARCH%\awealloc.sys" "%DRVDIR%\awealloc.sys" >nul
if %errorlevel% neq 0 (echo Failed to copy awealloc.sys & exit /b 1)

copy /y "%SRCDIR%\deviodrv\%ARCH%\deviodrv.sys" "%DRVDIR%\deviodrv.sys" >nul
if %errorlevel% neq 0 (echo Failed to copy deviodrv.sys & exit /b 1)

:: Copy user-mode files
echo Copying user-mode files...
copy /y "%SRCDIR%\cli\%ARCH%\imdisk.exe" "%SYSDIR%\imdisk.exe" >nul
if %errorlevel% neq 0 (echo Failed to copy imdisk.exe & exit /b 1)

copy /y "%SRCDIR%\cpl\%ARCH%\imdisk.cpl" "%SYSDIR%\imdisk.cpl" >nul
if %errorlevel% neq 0 (echo Failed to copy imdisk.cpl & exit /b 1)

copy /y "%SRCDIR%\svc\%ARCH%\imdsksvc.exe" "%SYSDIR%\imdsksvc.exe" >nul
if %errorlevel% neq 0 (echo Failed to copy imdsksvc.exe & exit /b 1)

copy /y "%SRCDIR%\uninstall_imdisk_new.cmd" "%SYSDIR%\uninstall_imdisk.cmd" >nul

:: Copy INF file
copy /y "%SRCDIR%\imdisk.inf" "%INFDIR%\imdisk.inf" >nul

:: Create kernel driver services
echo Creating driver services...

sc create DevIoDrv type= kernel start= auto binPath= "%DRVDIR%\deviodrv.sys" DisplayName= "DevIO Client Driver" >nul 2>&1
sc description DevIoDrv "Client driver for ImDisk devio proxy mode" >nul 2>&1

sc create AWEAlloc type= kernel start= auto binPath= "%DRVDIR%\awealloc.sys" DisplayName= "AWE Memory Allocation Driver" >nul 2>&1
sc description AWEAlloc "Driver for physical memory allocation through AWE" >nul 2>&1

sc create ImDisk type= kernel start= auto binPath= "%DRVDIR%\imdisk.sys" DisplayName= "ImDisk Virtual Disk Driver" >nul 2>&1
sc description ImDisk "Disk emulation driver" >nul 2>&1

:: Create user-mode service
sc create ImDskSvc type= own start= auto binPath= "%SYSDIR%\imdsksvc.exe" DisplayName= "ImDisk Virtual Disk Driver Helper" >nul 2>&1
sc description ImDskSvc "Helper service for ImDisk Virtual Disk Driver." >nul 2>&1

:: Add registry entries
echo Adding registry entries...

:: Uninstall entry
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "DisplayName" /t REG_SZ /d "ImDisk Virtual Disk Driver" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "Publisher" /t REG_SZ /d "LTR Data" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "HelpLink" /t REG_SZ /d "http://ltr-data.se/opencode.html" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "DisplayVersion" /t REG_SZ /d "*" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "UninstallString" /t REG_EXPAND_SZ /d "\"%%SystemRoot%%\system32\cmd.exe\" /c \"%%SystemRoot%%\system32\uninstall_imdisk.cmd\"" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "DisplayIcon" /t REG_SZ /d "%SYSDIR%\imdisk.cpl" /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "EstimatedSize" /t REG_DWORD /d 320 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "NoRepair" /t REG_DWORD /d 1 /f >nul
reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ImDisk" /v "NoModify" /t REG_DWORD /d 1 /f >nul

:: Shell extensions
reg add "HKCR\*\shell\ImDiskMountFile" /ve /t REG_SZ /d "Mount as ImDisk Virtual Disk" /f >nul
reg add "HKCR\*\shell\ImDiskMountFile\command" /ve /t REG_SZ /d "rundll32.exe imdisk.cpl,RunDLL_MountFile %%L" /f >nul

reg add "HKCR\Drive\shell\ImDiskUnmount" /ve /t REG_SZ /d "Unmount ImDisk Virtual Disk" /f >nul
reg add "HKCR\Drive\shell\ImDiskUnmount\command" /ve /t REG_SZ /d "rundll32.exe imdisk.cpl,RunDLL_RemoveDevice %%L" /f >nul

reg add "HKCR\Drive\shell\ImDiskSaveImage" /ve /t REG_SZ /d "Save disk contents as image file" /f >nul
reg add "HKCR\Drive\shell\ImDiskSaveImage\command" /ve /t REG_SZ /d "rundll32.exe imdisk.cpl,RunDLL_SaveImageFile %%L" /f >nul

:: Start services
echo Starting services...
sc start DevIoDrv >nul 2>&1
sc start AWEAlloc >nul 2>&1
sc start ImDisk >nul 2>&1
sc start ImDskSvc >nul 2>&1

echo.
echo ImDisk Virtual Disk Driver installed successfully.
echo.

endlocal
exit /b 0
