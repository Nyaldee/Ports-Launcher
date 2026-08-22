@echo off
setlocal EnableExtensions EnableDelayedExpansion
color 02
:: >nul 2>&1 -- peut echouer sans gravite selon comment la console a ete
:: lancee (certains hotes de terminal refusent un redimensionnement) --
:: pas la peine de le signaler au joueur.
mode con cols=90 lines=35

cd /d "%~dp0"
set "ROOT=%~dp0"
set "PY=%ROOT%zelda3_python\python.exe"
set "RESTOOL=%ROOT%zelda3_python\restool.py"
set "GAME=%ROOT%zelda3.exe"
set "INI=%ROOT%zelda3.ini"
set "ROM=%ROOT%zelda3.sfc"

:: A modifier ici -- chemin de la ROM pour chaque langue disponible (memes
:: codes que dans les lignes commentees de zelda3.ini). Laisser vide ou un
:: nom de fichier absent pour une langue que tu n'as pas.
set "ROM_DE=german.sfc"
set "ROM_FR=french.sfc"
set "ROM_FR_C=french_canada.sfc"
set "ROM_ES=spanish.sfc"
set "ROM_PL=polish.sfc"
set "ROM_PT=portuguese.sfc"
set "ROM_REDUX=redux.sfc"
set "ROM_NL=dutch.sfc"
set "ROM_SV=swedish.sfc"

:: En ligne plutot qu'en sous-routine partagee -- un exit /b dans un
:: label appele ne termine que l'appel, pas tout le script.
if not exist "%GAME%" (
    echo.
    echo ERROR: zelda3.exe not found.
    echo.
    pause
    exit 1
)
if not exist "%PY%" (
    echo.
    echo ERROR: zelda3_python\python.exe not found.
    echo.
    pause
    exit 1
)

:MENU
call :HEADER "Zelda 3 - A Link to the Past"
echo   [1] Play
echo   [2] Add/change language
echo   [3] Extract dialogue text
echo   [4] Rebuild dialogue text
echo   [5] Rebuild zelda3_assets.dat (English only)
echo.
echo   [Q] Quit
echo.
choice /c 12345Q /n /m "Choice: "
if errorlevel 6 exit 0
if errorlevel 5 goto REBUILD_BASE
if errorlevel 4 goto REBUILD_TEXT
if errorlevel 3 goto EXTRACT_TEXT
if errorlevel 2 goto LANGUAGE
goto PLAY

:PLAY
cls
echo.
echo Launching Zelda 3...
echo.
:: zelda3.exe est compile en subsystem console (pas GUI) -- Windows lui
:: alloue toujours sa propre fenetre console au demarrage, quelle que soit
:: la maniere dont on le lance depuis ce .bat ; un simple "start" ne suffit
:: pas. PowerShell (present nativement, contrairement a zelda3_python qui
:: est facultatif) via ProcessStartInfo.CreateNoWindow empeche cette
:: fenetre parasite d'apparaitre.
powershell -NoProfile -Command "$p = New-Object System.Diagnostics.ProcessStartInfo; $p.FileName = '%GAME%'; $p.UseShellExecute = $false; $p.CreateNoWindow = $true; [System.Diagnostics.Process]::Start($p) | Out-Null"
exit 0

:REBUILD_BASE
call :HEADER "REBUILD ZELDA3_ASSETS.DAT (ENGLISH ONLY)"
if not exist "%ROM%" (
    echo.
    echo ERROR: zelda3.sfc not found in this folder.
    echo.
    pause
    goto MENU
)

echo.
echo Rebuilding zelda3_assets.dat from your English ROM...
echo.
if not exist "%ROOT%zelda3_python\sprites" mkdir "%ROOT%zelda3_python\sprites"
"%PY%" "%RESTOOL%" --extract-from-rom -r "%ROM%"
if errorlevel 1 (
    echo.
    echo BUILD FAILED.
    echo.
    pause
    goto MENU
)

call :CLEANUP_TOOL_FOLDER
echo.
echo Done! zelda3_assets.dat rebuilt (English only, no foreign language included).
echo.
pause
goto MENU

:LANGUAGE
call :HEADER "ADD/CHANGE LANGUAGE"
echo   [1] German  (de)      -- "%ROM_DE%"
echo   [2] French  (fr)      -- "%ROM_FR%"
echo   [3] French Canada (fr-c) -- "%ROM_FR_C%"
echo   [4] Spanish (es)      -- "%ROM_ES%"
echo   [5] Polish  (pl)      -- "%ROM_PL%"
echo   [6] Portuguese (pt)   -- "%ROM_PT%"
echo   [7] Redux            -- "%ROM_REDUX%"
echo   [8] Dutch   (nl)      -- "%ROM_NL%"
echo   [9] Swedish (sv)      -- "%ROM_SV%"
echo.
echo   [B] Back
echo.
echo (Edit the ROM paths at the top of this .bat file to change them.)
echo.
choice /c 123456789B /n /m "Choice: "
if errorlevel 10 goto MENU
if errorlevel 9 ( set "FOREIGN_ROM=%ROM_SV%" & set "LANGCODE=sv" & goto DO_LANGUAGE )
if errorlevel 8 ( set "FOREIGN_ROM=%ROM_NL%" & set "LANGCODE=nl" & goto DO_LANGUAGE )
if errorlevel 7 ( set "FOREIGN_ROM=%ROM_REDUX%" & set "LANGCODE=redux" & goto DO_LANGUAGE )
if errorlevel 6 ( set "FOREIGN_ROM=%ROM_PT%" & set "LANGCODE=pt" & goto DO_LANGUAGE )
if errorlevel 5 ( set "FOREIGN_ROM=%ROM_PL%" & set "LANGCODE=pl" & goto DO_LANGUAGE )
if errorlevel 4 ( set "FOREIGN_ROM=%ROM_ES%" & set "LANGCODE=es" & goto DO_LANGUAGE )
if errorlevel 3 ( set "FOREIGN_ROM=%ROM_FR_C%" & set "LANGCODE=fr-c" & goto DO_LANGUAGE )
if errorlevel 2 ( set "FOREIGN_ROM=%ROM_FR%" & set "LANGCODE=fr" & goto DO_LANGUAGE )
set "FOREIGN_ROM=%ROM_DE%" & set "LANGCODE=de"

:DO_LANGUAGE
if not exist "%FOREIGN_ROM%" (
    echo.
    echo ERROR: "%FOREIGN_ROM%" not found.
    echo.
    pause
    goto MENU
)
if not exist "%ROM%" (
    echo.
    echo ERROR: zelda3.sfc not found in this folder.
    echo.
    pause
    goto MENU
)

echo.
echo Step 1/3: Rebuilding base game data from your English ROM...
echo.
:: restool.py n'a pas de os.makedirs pour celui-la (contrairement a
:: img/overworld/dungeon/sound) -- il compte sur ce dossier deja present
:: dans son propre depot. On ne distribue rien lie aux sprites ici, donc
:: on le cree nous-memes juste avant, vide, jamais commite.
if not exist "%ROOT%zelda3_python\sprites" mkdir "%ROOT%zelda3_python\sprites"
"%PY%" "%RESTOOL%" --extract-from-rom -r "%ROM%"
if errorlevel 1 (
    echo.
    echo EXTRACTION FAILED.
    echo.
    pause
    goto MENU
)

echo.
echo Step 2/3: Extracting dialogue from "%FOREIGN_ROM%"...
echo.
"%PY%" "%RESTOOL%" --extract-dialogue -r "%FOREIGN_ROM%" --force --assume-language=%LANGCODE%
if errorlevel 1 (
    echo.
    echo DIALOGUE EXTRACTION FAILED. Is this really a Zelda 3 ROM?
    echo.
    pause
    goto MENU
)

echo.
echo Step 3/3: Rebuilding zelda3_assets.dat with "%LANGCODE%"...
echo.
"%PY%" "%RESTOOL%" --languages=%LANGCODE%
if errorlevel 1 (
    echo.
    echo BUILD FAILED.
    echo.
    pause
    goto MENU
)

:: PowerShell plutot qu'une substitution en batch pur -- editer un fichier
:: ligne par ligne en .bat est fragile (voir les mesaventures d'encodage du
:: script OpenGOAL), alors que powershell.exe est present sur tout Windows
:: et fait un remplacement fiable en une commande.
powershell -NoProfile -Command "(Get-Content '%INI%') -replace '^#?\s*Language\s*=.*$', 'Language = %LANGCODE%' | Set-Content '%INI%'"

call :CLEANUP_TOOL_FOLDER

echo.
echo Done! Language set to "%LANGCODE%".
echo.
pause
goto MENU

:EXTRACT_TEXT
call :HEADER "EXTRACT DIALOGUE TEXT"
echo Which ROM's dialogue do you want to extract for editing?
echo.
echo   [0] English (us)      -- "%ROM%"
echo   [1] German  (de)      -- "%ROM_DE%"
echo   [2] French  (fr)      -- "%ROM_FR%"
echo   [3] French Canada (fr-c) -- "%ROM_FR_C%"
echo   [4] Spanish (es)      -- "%ROM_ES%"
echo   [5] Polish  (pl)      -- "%ROM_PL%"
echo   [6] Portuguese (pt)   -- "%ROM_PT%"
echo   [7] Redux            -- "%ROM_REDUX%"
echo   [8] Dutch   (nl)      -- "%ROM_NL%"
echo   [9] Swedish (sv)      -- "%ROM_SV%"
echo.
echo   [B] Back
echo.
echo (Edit the ROM paths at the top of this .bat file to change them.)
echo.
choice /c 0123456789B /n /m "Choice: "
if errorlevel 11 goto MENU
if errorlevel 10 ( set "TEXT_ROM=%ROM_SV%" & set "TEXT_LANG=sv" & goto DO_EXTRACT_TEXT )
if errorlevel 9 ( set "TEXT_ROM=%ROM_NL%" & set "TEXT_LANG=nl" & goto DO_EXTRACT_TEXT )
if errorlevel 8 ( set "TEXT_ROM=%ROM_REDUX%" & set "TEXT_LANG=redux" & goto DO_EXTRACT_TEXT )
if errorlevel 7 ( set "TEXT_ROM=%ROM_PT%" & set "TEXT_LANG=pt" & goto DO_EXTRACT_TEXT )
if errorlevel 6 ( set "TEXT_ROM=%ROM_PL%" & set "TEXT_LANG=pl" & goto DO_EXTRACT_TEXT )
if errorlevel 5 ( set "TEXT_ROM=%ROM_ES%" & set "TEXT_LANG=es" & goto DO_EXTRACT_TEXT )
if errorlevel 4 ( set "TEXT_ROM=%ROM_FR_C%" & set "TEXT_LANG=fr-c" & goto DO_EXTRACT_TEXT )
if errorlevel 3 ( set "TEXT_ROM=%ROM_FR%" & set "TEXT_LANG=fr" & goto DO_EXTRACT_TEXT )
if errorlevel 2 ( set "TEXT_ROM=%ROM_DE%" & set "TEXT_LANG=de" & goto DO_EXTRACT_TEXT )
set "TEXT_ROM=%ROM%" & set "TEXT_LANG=us"

:DO_EXTRACT_TEXT
if not exist "%TEXT_ROM%" (
    echo.
    echo ERROR: "%TEXT_ROM%" not found.
    echo.
    pause
    goto MENU
)

:: --extract-dialogue appelle uniquement decode_font() + print_dialogue()
:: (voir restool.py) -- ni l'un ni l'autre n'a besoin des donnees de base
:: (donjons/overworld/son) que produit --extract-from-rom, pas la peine de
:: la lancer ici (contrairement a Rebuild, qui recompile TOUT le fichier).
echo.
echo Extracting dialogue from "%TEXT_ROM%"...
echo.
:: Meme principe que le changement de langue -- restool.py identifie la
:: langue lui-meme via le hash de la ROM, on capture juste sa sortie pour
:: savoir quel fichier dialogue_xx.txt il vient d'ecrire. --assume-language
:: couvre le cas d'un hash non reconnu (ex: version mise a jour d'un hack) --
:: restool.py ecrit alors dialogue_xx.txt/font_xx.png pour CE code plutot
:: que de retomber sur les fichiers "us" par defaut.
set "LOGFILE=%TEMP%\zelda3_extract_%RANDOM%.log"
"%PY%" "%RESTOOL%" --extract-dialogue -r "%TEXT_ROM%" --force --assume-language=%TEXT_LANG% > "%LOGFILE%" 2>&1
set "EXTRACT_ERR=%errorlevel%"
type "%LOGFILE%"
if not "%EXTRACT_ERR%"=="0" (
    del "%LOGFILE%" >nul 2>&1
    echo.
    echo DIALOGUE EXTRACTION FAILED. Is this really a Zelda 3 ROM?
    echo.
    pause
    goto MENU
)
set "TEXTCODE="
for /f "tokens=4 delims= " %%L in ('findstr /b /c:"Identified ROM as" "%LOGFILE%"') do set "TEXTCODE=%%L"
if "%TEXTCODE%"=="" if not "%TEXT_LANG%"=="us" set "TEXTCODE=%TEXT_LANG%"
del "%LOGFILE%" >nul 2>&1

set "DIALOGUE_FILE=dialogue.txt"
set "FONT_FILE=font.png"
if not "%TEXTCODE%"=="us" if not "%TEXTCODE%"=="" (
    set "DIALOGUE_FILE=dialogue_%TEXTCODE:-=_%.txt"
    set "FONT_FILE=font_%TEXTCODE:-=_%.png"
)

:: zelda3_python\ reste juste l'outil -- les fichiers a editer/conserver
:: vont a la racine du port, a cote de zelda3.exe, pas dans le dossier de
:: l'interpreteur. font_xx.png est aussi genere par --extract-dialogue
:: (decode_font(), voir sprite_sheets.py) et relu plus tard par la
:: compilation (encode_font_from_png) -- meme sort que le fichier dialogue,
:: sinon le nettoyage qui suit l'effacerait avant la reconstruction.
move /y "%ROOT%zelda3_python\%DIALOGUE_FILE%" "%ROOT%%DIALOGUE_FILE%" >nul
move /y "%ROOT%zelda3_python\%FONT_FILE%" "%ROOT%%FONT_FILE%" >nul
call :CLEANUP_TOOL_FOLDER

echo.
echo Done! Edit this file with a text editor, then use "Rebuild after
echo editing dialogue text" from the menu:
echo   %ROOT%%DIALOGUE_FILE%
echo.
pause
goto MENU

:REBUILD_TEXT
call :HEADER "REBUILD AFTER EDITING DIALOGUE TEXT"
echo Which language do you want to rebuild?
echo.
if exist "%ROOT%dialogue.txt" (echo   [0] English ^(us^) -- extracted) else (echo   [0] English ^(us^) -- not extracted)
if exist "%ROOT%dialogue_de.txt" (echo   [1] German  ^(de^) -- extracted) else (echo   [1] German  ^(de^) -- not extracted)
if exist "%ROOT%dialogue_fr.txt" (echo   [2] French  ^(fr^) -- extracted) else (echo   [2] French  ^(fr^) -- not extracted)
if exist "%ROOT%dialogue_fr_c.txt" (echo   [3] French Canada ^(fr-c^) -- extracted) else (echo   [3] French Canada ^(fr-c^) -- not extracted)
if exist "%ROOT%dialogue_es.txt" (echo   [4] Spanish ^(es^) -- extracted) else (echo   [4] Spanish ^(es^) -- not extracted)
if exist "%ROOT%dialogue_pl.txt" (echo   [5] Polish  ^(pl^) -- extracted) else (echo   [5] Polish  ^(pl^) -- not extracted)
if exist "%ROOT%dialogue_pt.txt" (echo   [6] Portuguese ^(pt^) -- extracted) else (echo   [6] Portuguese ^(pt^) -- not extracted)
if exist "%ROOT%dialogue_redux.txt" (echo   [7] Redux -- extracted) else (echo   [7] Redux -- not extracted)
if exist "%ROOT%dialogue_nl.txt" (echo   [8] Dutch   ^(nl^) -- extracted) else (echo   [8] Dutch   ^(nl^) -- not extracted)
if exist "%ROOT%dialogue_sv.txt" (echo   [9] Swedish ^(sv^) -- extracted) else (echo   [9] Swedish ^(sv^) -- not extracted)
echo.
echo   [A] All extracted languages at once
echo   [B] Back
echo.
choice /c 0123456789AB /n /m "Choice: "
if errorlevel 12 goto MENU
if errorlevel 11 goto REBUILD_SCAN_ALL
if errorlevel 10 ( set "ONLY_CODE=sv" & set "ONLY_FILE=dialogue_sv.txt" & goto REBUILD_ONE )
if errorlevel 9 ( set "ONLY_CODE=nl" & set "ONLY_FILE=dialogue_nl.txt" & goto REBUILD_ONE )
if errorlevel 8 ( set "ONLY_CODE=redux" & set "ONLY_FILE=dialogue_redux.txt" & goto REBUILD_ONE )
if errorlevel 7 ( set "ONLY_CODE=pt" & set "ONLY_FILE=dialogue_pt.txt" & goto REBUILD_ONE )
if errorlevel 6 ( set "ONLY_CODE=pl" & set "ONLY_FILE=dialogue_pl.txt" & goto REBUILD_ONE )
if errorlevel 5 ( set "ONLY_CODE=es" & set "ONLY_FILE=dialogue_es.txt" & goto REBUILD_ONE )
if errorlevel 4 ( set "ONLY_CODE=fr-c" & set "ONLY_FILE=dialogue_fr_c.txt" & goto REBUILD_ONE )
if errorlevel 3 ( set "ONLY_CODE=fr" & set "ONLY_FILE=dialogue_fr.txt" & goto REBUILD_ONE )
if errorlevel 2 ( set "ONLY_CODE=de" & set "ONLY_FILE=dialogue_de.txt" & goto REBUILD_ONE )
set "ONLY_CODE=us" & set "ONLY_FILE=dialogue.txt"

:REBUILD_ONE
if not exist "%ROOT%%ONLY_FILE%" (
    echo.
    echo ERROR: "%ONLY_FILE%" not found -- use "Extract dialogue text" first.
    echo.
    pause
    goto MENU
)
set "FOUND_ANY=1"
if "%ONLY_CODE%"=="us" (set "LANGLIST=") else (set "LANGLIST=%ONLY_CODE%")
goto DO_REBUILD

:REBUILD_SCAN_ALL
:: Un par langue trouvee -- reconstruit la liste --languages= a partir des
:: noms de fichiers, meme principe que le changement de langue, sans
:: redemander les codes qu'on connait deja.
set "FOUND_ANY="
set "LANGLIST="
if exist "%ROOT%dialogue.txt" set "FOUND_ANY=1"
for %%C in (de fr es pl pt redux nl sv) do (
    if exist "%ROOT%dialogue_%%C.txt" (
        set "FOUND_ANY=1"
        if "!LANGLIST!"=="" (set "LANGLIST=%%C") else (set "LANGLIST=!LANGLIST!,%%C")
    )
)
if exist "%ROOT%dialogue_fr_c.txt" (
    set "FOUND_ANY=1"
    if "!LANGLIST!"=="" (set "LANGLIST=fr-c") else (set "LANGLIST=!LANGLIST!,fr-c")
)

if not defined FOUND_ANY (
    echo.
    echo ERROR: No dialogue*.txt found at the root of this folder.
    echo Use "Extract dialogue text" first.
    echo.
    pause
    goto MENU
)

:DO_REBUILD
if not exist "%ROM%" (
    echo.
    echo ERROR: zelda3.sfc not found in this folder.
    echo.
    pause
    goto MENU
)

echo.
echo Step 1/2: Rebuilding base game data from your English ROM...
echo.
if not exist "%ROOT%zelda3_python\sprites" mkdir "%ROOT%zelda3_python\sprites"
"%PY%" "%RESTOOL%" --extract-from-rom -r "%ROM%"
if errorlevel 1 (
    echo.
    echo EXTRACTION FAILED.
    echo.
    pause
    goto MENU
)

:: Remet les fichiers edites a l'endroit ou restool.py les attend (a cote
:: de lui, pas a la racine du port) juste avant de compiler. font.png (us)
:: n'a pas besoin d'etre restaure : --extract-from-rom au-dessus vient deja
:: de le regenerer (decode_font(), voir extract_resources.main()) -- mais
:: font_xx.png des langues etrangeres n'existe QUE via --extract-dialogue,
:: jamais touche par cette etape, donc a restaurer comme le dialogue.
if exist "%ROOT%dialogue.txt" copy /y "%ROOT%dialogue.txt" "%ROOT%zelda3_python\dialogue.txt" >nul
for %%C in (de fr es pl pt redux nl sv) do (
    if exist "%ROOT%dialogue_%%C.txt" copy /y "%ROOT%dialogue_%%C.txt" "%ROOT%zelda3_python\dialogue_%%C.txt" >nul
    if exist "%ROOT%font_%%C.png" copy /y "%ROOT%font_%%C.png" "%ROOT%zelda3_python\font_%%C.png" >nul
)
if exist "%ROOT%dialogue_fr_c.txt" copy /y "%ROOT%dialogue_fr_c.txt" "%ROOT%zelda3_python\dialogue_fr_c.txt" >nul
if exist "%ROOT%font_fr_c.png" copy /y "%ROOT%font_fr_c.png" "%ROOT%zelda3_python\font_fr_c.png" >nul

echo.
echo Step 2/2: Rebuilding zelda3_assets.dat with "%LANGLIST%"...
echo.
if "%LANGLIST%"=="" (
    "%PY%" "%RESTOOL%"
) else (
    "%PY%" "%RESTOOL%" --languages=%LANGLIST%
)
if errorlevel 1 (
    echo.
    echo BUILD FAILED.
    echo.
    pause
    goto MENU
)

call :CLEANUP_TOOL_FOLDER
echo.
echo Done!
echo.
pause
goto MENU

:: zelda3_python\ n'est que l'outil (interpreteur + scripts) -- restool.py
:: y ecrit ses fichiers de travail temporaires parce qu'il tourne depuis
:: son propre dossier (os.chdir), pas pour qu'on les y garde. Balaye tout
:: apres coup ; zelda3_assets.dat, le seul fichier qui compte, est deja
:: ecrit un niveau au-dessus (dans le dossier du jeu), jamais touche ici.
:CLEANUP_TOOL_FOLDER
for %%D in (dungeon overworld img sound sprites __pycache__) do (
    if exist "%ROOT%zelda3_python\%%D" rd /s /q "%ROOT%zelda3_python\%%D"
)
del /q "%ROOT%zelda3_python\dialogue*.txt" "%ROOT%zelda3_python\generated_*.h" "%ROOT%zelda3_python\linksprite.png" "%ROOT%zelda3_python\map32_to_map16.txt" "%ROOT%zelda3_python\music_info.yaml" "%ROOT%zelda3_python\sfx.txt" "%ROOT%zelda3_python\hud_icons.png" "%ROOT%zelda3_python\font*.png" "%ROOT%zelda3_python\sound_*.txt" >nul 2>&1
exit /b 0

:HEADER
cls
echo.
echo ============================================================
echo   %~1
echo ============================================================
echo.
exit /b 0
