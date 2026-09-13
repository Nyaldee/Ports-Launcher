@echo off
setlocal EnableExtensions
title OpenGOAL for Ports Launcher & Color 0E
mode con cols=90 lines=35

cd /d "%~dp0"
set "ROOT=%~dp0"
set "GK=%ROOT%gk.exe"
set "EXTRACTOR=%ROOT%extractor.exe"

:: En ligne plutot qu'en sous-routine partagee -- un exit /b dans un
:: label appele ne termine que l'appel, pas tout le script.
if not exist "%GK%" (
    echo.
    echo ERROR: gk.exe not found.
    echo.
    pause
    exit 1
)
if not exist "%EXTRACTOR%" (
    echo.
    echo ERROR: extractor.exe not found.
    echo.
    pause
    exit 1
)

:MENU
call :HEADER "OpenGOAL for Ports Launcher"
echo   [1] Jak and Daxter
echo   [2] Jak II
echo   [3] Jak 3
echo.
echo   [I] Install / extract a game
echo   [Q] Quit
echo.
choice /c 123IQ /n /m "Choice: "
if errorlevel 5 exit 0
if errorlevel 4 goto INSTALL_MENU
call :SET_GAME %errorlevel%
goto GAME_MENU

:GAME_MENU
call :HEADER "%GAMENAME%"
call :CHECK_INSTALLED
echo Status: %INSTALL_STATUS%
echo.
echo   [1] Play
echo   [2] Install / extract
echo   [3] Back
echo.
choice /c 123 /n /m "Choice: "
if errorlevel 3 goto MENU
if errorlevel 2 goto INSTALL
goto PLAY

:INSTALL_MENU
call :HEADER "INSTALL A GAME"
echo   [1] Jak and Daxter
echo   [2] Jak II
echo   [3] Jak 3
echo   [4] Back
echo.
choice /c 1234 /n /m "Choice: "
if errorlevel 4 goto MENU
call :SET_GAME %errorlevel%
goto INSTALL

:INSTALL
call :HEADER "INSTALL: %GAMENAME%"
echo Enter the full path to your ISO (drag-and-drop works too).
echo.
set "ISO="
set /p "ISO=ISO: "
set "ISO=%ISO:"=%"

if not exist "%ISO%" (
    echo.
    echo ERROR: ISO not found.
    echo.
    pause
    goto GAME_MENU
)

echo.
echo ISO: "%ISO%"
echo.
echo Extracting...
echo.

:: Doit s'executer depuis le dossier OpenGOAL lui-meme -- requis par
:: extractor.exe.
"%EXTRACTOR%" --game %GAME% "%ISO%"
if errorlevel 1 (
    echo.
    echo EXTRACTION FAILED.
    echo.
    pause
    goto GAME_MENU
)

echo.
echo Extraction complete. You can now play %GAMENAME% from the menu.
echo.
pause
goto GAME_MENU

:PLAY
cls
echo.
echo Launching %GAMENAME%...
echo.
:: Processus detache -- cette fenetre peut se fermer des que gk.exe demarre.
start "" "%GK%" --portable --game %GAME%
exit 0

:: OpenGOAL range les donnees de jeu sous iso_data/<game> ou
:: data/decompiler_out/<game> -- verifie directement plutot que via un
:: fichier marqueur separe ; gk.exe reste l'autorite finale si une
:: installation non standard echappe a cette verification.
:CHECK_INSTALLED
set "INSTALL_STATUS=NOT INSTALLED"
if exist "%ROOT%iso_data\%GAME%" set "INSTALL_STATUS=INSTALLED"
if exist "%ROOT%data\decompiler_out\%GAME%" set "INSTALL_STATUS=INSTALLED"
exit /b 0

:: %1 est l'index du choix (1/2/3), partage par MENU et INSTALL_MENU
:: pour ne maintenir la liste des jeux qu'a un seul endroit.
:SET_GAME
if "%~1"=="1" (set "GAME=jak1" & set "GAMENAME=Jak and Daxter")
if "%~1"=="2" (set "GAME=jak2" & set "GAMENAME=Jak II")
if "%~1"=="3" (set "GAME=jak3" & set "GAMENAME=Jak 3")
exit /b 0

:HEADER
cls
echo.
echo ============================================================
echo   %~1
echo ============================================================
echo.
exit /b 0
