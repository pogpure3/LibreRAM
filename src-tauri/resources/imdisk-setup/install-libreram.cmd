@echo off
:: LibreRAM silent ImDisk install (fork 3.0.3 layout: sys/cli/cpl/svc).
:: No INF: rundll32 InstallHinfSection pops the "Files Needed" dialog when
:: run from a resource directory; plain copies + sc do not.
setlocal
set ARCH=amd64
set SRCDIR=%~dp0
set SRCDIR=%SRCDIR:~0,-1%
set SYSDIR=%SystemRoot%\System32
set DRVDIR=%SystemRoot%\System32\drivers
set INFDIR=%SystemRoot%\inf

net stop ImDskSvc >nul 2>&1
net stop ImDisk >nul 2>&1

copy /y "%SRCDIR%\sys\%ARCH%\imdisk.sys" "%DRVDIR%\imdisk.sys" >nul || (echo imdisk.sys copy failed & exit /b 1)
copy /y "%SRCDIR%\cli\%ARCH%\imdisk.exe" "%SYSDIR%\imdisk.exe" >nul || (echo imdisk.exe copy failed & exit /b 1)
copy /y "%SRCDIR%\cpl\%ARCH%\imdisk.cpl" "%SYSDIR%\imdisk.cpl" >nul || (echo imdisk.cpl copy failed & exit /b 1)
copy /y "%SRCDIR%\svc\%ARCH%\imdsksvc.exe" "%SYSDIR%\imdsksvc.exe" >nul || (echo imdsksvc.exe copy failed & exit /b 1)
copy /y "%SRCDIR%\imdisk.inf" "%INFDIR%\imdisk.inf" >nul 2>&1

sc create ImDisk type= kernel start= auto binPath= "%DRVDIR%\imdisk.sys" DisplayName= "ImDisk Virtual Disk Driver" >nul 2>&1
sc description ImDisk "Disk emulation driver" >nul 2>&1
sc create ImDskSvc type= own start= auto binPath= "%SYSDIR%\imdsksvc.exe" DisplayName= "ImDisk Virtual Disk Driver Helper" >nul 2>&1
sc description ImDskSvc "Helper service for ImDisk Virtual Disk Driver." >nul 2>&1

sc start ImDisk >nul 2>&1
sc start ImDskSvc >nul 2>&1

exit /b 0
