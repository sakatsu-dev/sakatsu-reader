<#
.SYNOPSIS
    Launch the reader against a fully isolated profile, for a desktop acceptance checkpoint.

.DESCRIPTION
    Desktop acceptance needs a person to look at the app. This prepares the conditions so that
    looking is quick, and so that nobody has to risk their own saved words to do it.

    ## What "isolated" means here, exactly

    Redirecting APPDATA alone is not enough, and an earlier version of this script did only that.
    `resource_dir` in src-tauri/src/lib.rs resolves in this order:

        SAKATSU_*_DIR override  ->  device_data_dir()/<name>  ->  app_data_dir()/<name>

    and `device_data_dir()` reads SAKATSU_DEVICE_DATA_DIR, then a `device-data-location.txt` under
    APPDATA, then LOCALAPPDATA. So a script that moves APPDATA and leaves LOCALAPPDATA alone sends
    the store to scratch and the models, dictionaries and kanji tables to the real machine.

    This moves APPDATA, LOCALAPPDATA and SAKATSU_DEVICE_DATA_DIR together.

    ## Resources are copied, not borrowed

    Pointing SAKATSU_DICT_DIR at the real dictionaries was also wrong: `dictionaries_install_dir()`
    *is* `dictionaries_dir()`, so an import inside the checkpoint would have written into them. A
    "read only" comment enforces nothing. The dictionaries and kanji table are copied into the
    scratch profile instead -- about half a gigabyte, once, and the profile is kept by default
    so `-Reuse` can skip the copy next time.

    Where they are copied *from* is not assumed: `cargo run --bin resolve-resources` reports the
    paths the app itself would resolve, because the live directory may be the device-data one or
    the legacy roaming one depending on which currently holds files.

    ## Two store conditions, and they are genuinely different

      -Store Empty        a fresh directory. `Store::open` calls `create_dir_all`, so it is created.
                          The reader works, every word is unmarked, and NO warning is shown.
      -Store Unavailable  a *file* where the store directory belongs, so `create_dir_all` fails and
                          `Store::open` returns an error. The reader works, every word is unmarked,
                          and the warning IS shown.

    Renaming your store directory tests neither: it produces the empty case while risking real
    data. That mistake is why this script exists.

.PARAMETER Scenario
    Which acceptance rows this launch is for. The default prepares nothing beyond the resources,
    which is the reading and store checkpoint this script was written for.

      reading         (default) the reading, marking and guess-mark rows
      recovery        three dictionary containers left mid-transaction: one to restore, one to
                      keep, and one whose restoration fails until its obstruction is removed
      loading         an interrupted install whose restored components load, need no loading, or
                      cannot be established
      loading-failed  the same, with a kanji table that comes back empty: the loading FAILURE row
      relocation      a move target inside the profile, for the move and deferred-cleanup rows

    **The store rows alone do not establish recovery or relocation acceptance**, which is why these
    exist: an empty profile has nothing outstanding to look at, so those panels never appear.

    Every fixture is planted by `cargo run --bin checkpoint-fixture`, which builds them by running
    the production primitives and stopping -- a hand-written journal is a shape that looks right, and
    this repository has already paid for one of those.

.PARAMETER Store
    Empty (default) or Unavailable.

.PARAMETER PrepareOnly
    Prepare the profile and print the checklist, without launching. What it is for: confirming that
    a scenario's fixtures were actually planted before spending a launch on looking at them, and
    preparing on one occasion to observe on another. The profile is kept and the printed -Reuse
    command launches against it.

.PARAMETER Clean
    Delete the scratch profile on exit. Off by default, because verifying that a marked word
    survives a restart means launching twice against the same profile.

.PARAMETER Reuse
    Reuse an existing scratch profile by name instead of creating a new one -- the second launch of
    a persistence check.

.PARAMETER Analyzer
    Pin SAKATSU_ANALYZER for this run: `lindera` or `ichiran`. Those are the only two values
    `choose_analyzer` answers to. Unset by default, which leaves the app to choose exactly as it
    ships -- and note that unset is not the same as any value: with the variable set to anything at
    all the saved setting stops being consulted.

    **Which engine answered decides several of the rows below, and it is not obvious from the
    window.** `choose_analyzer(Auto)` probes Ichiran and uses it if it answers, and every
    colloquial composition on this branch -- explanatory nan, contracted motion, the suffix
    adjective -- is a Lindera post-pass. So on a machine with Ichiran installed the default sends
    rows 2 and 3 to an engine that does not implement them, and they fail for a reason that has
    nothing to do with what is being checked. Three launches were spent on exactly that.

    Whatever is chosen, the engine is printed below and belongs in the recorded observation.

.PARAMETER AnalyzerFault
    Make the analyzer fail on purpose, deterministically, to observe the visible fallback:
    `unavailable`, `timeout`, `malformed`, `spans` (an answer the span contract refuses), `both`
    (the built-in retry fails too, on every passage), or `first-then-both` (the first passage is
    read by the built-in analyzer, and every later one fails on both engines -- the sequence in
    which a previous passage's survival can actually be observed). Sets SAKATSU_ANALYZER_FAULT for
    this launch only.

    **The sidecar is never contacted.** The route's primary claims to be Ichiran and fails in the
    named way, so the reader's own Ichiran is neither damaged nor stopped to manufacture a failure.
    Honoured by debug builds only -- `cargo tauri dev`, which is what this script launches -- and
    ignored entirely by a release build. Not recorded in the profile, and not carried into the
    printed reuse commands: a persistence relaunch should see the ordinary route.

.EXAMPLE
    pwsh -File scripts/desktop-checkpoint.ps1
    pwsh -File scripts/desktop-checkpoint.ps1 -Reuse sakatsu-checkpoint-Empty-20260908-141233
    pwsh -File scripts/desktop-checkpoint.ps1 -Store Unavailable
    pwsh -File scripts/desktop-checkpoint.ps1 -Analyzer lindera
    pwsh -File scripts/desktop-checkpoint.ps1 -AnalyzerFault timeout
#>
# PowerShell 7 or newer, and this is about *encoding*, not features.
#
# This launcher is ASCII. Localized checklist text is read explicitly as UTF-8.
# The Windows PowerShell codepage cannot reinterpret either code or Japanese.
#requires -Version 5.1

[CmdletBinding()]
param(
    [ValidateSet('Empty', 'Unavailable')]
    [string]$Store = 'Empty',
    [ValidateSet('reading', 'recovery', 'loading', 'loading-failed', 'relocation')]
    [string]$Scenario = 'reading',
    [switch]$Clean,
    [switch]$PrintChecklist,
    [switch]$PrepareOnly,
    [string]$Reuse,
    [ValidateSet('lindera', 'ichiran')]
    [string]$Analyzer,
    [ValidateSet('unavailable', 'timeout', 'malformed', 'spans', 'both', 'first-then-both')]
    [string]$AnalyzerFault
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# Every command this script prints has to carry the pin forward. Without it the relaunch that
# checks "the marked word is still marked" would run under the shipped default -- a different
# engine, a different segmentation, and therefore not necessarily the same token to look for.
$analyzerArg = if ($Analyzer) { " -Analyzer $Analyzer" } else { '' }
# Carried forward for the same reason: the relaunch that checks "the new location survived a
# restart" must not quietly prepare a different profile from the launch that moved it.
$scenarioArg = if ($Scenario -ne 'reading') { " -Scenario $Scenario" } else { '' }

# Keep executable text ASCII. The fixture is UTF-8 regardless of the host ANSI codepage.
$checkpointText = [System.IO.File]::ReadAllText(
    (Join-Path $PSScriptRoot 'desktop-checkpoint-text.json'),
    [System.Text.Encoding]::UTF8
) | ConvertFrom-Json
function Get-CheckpointText([string]$Key) {
    $property = $checkpointText.PSObject.Properties[$Key]
    if ($null -eq $property) { throw "Missing checkpoint text: $Key" }
    return [string]$property.Value
}
# Console output must also carry the Japanese without using the OEM codepage.
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }
if ($PrintChecklist) {
    $checkpointText.PSObject.Properties | Where-Object { $_.Name -like 'row_*' } |
        ForEach-Object { Get-CheckpointText $_.Name }
    exit 0
}

# --- Refuse to launch beside a running instance -----------------------------------------------
#
# The app registers `tauri_plugin_single_instance` *first*, before anything else, so a second copy
# forwards its arguments to the first and exits. A checkpoint launched while your ordinary app is
# open would therefore do nothing visible except hand the checkpoint to the app you were trying
# not to touch -- and the isolated profile would go unobserved while you looked at your real one.
$running = Get-Process -Name 'sakatsu-desktop', (Get-CheckpointText "legacy_process_name") -ErrorAction SilentlyContinue
if ($running) {
    $ids = ($running | ForEach-Object { "$($_.ProcessName) (pid $($_.Id))" }) -join ', '
    throw "Sakatsu is already running: $ids. Close it first -- single-instance forwarding would send this checkpoint to that window, not to the isolated profile."
}

# --- Where the real resources live, according to the app --------------------------------------
#
# Asked rather than guessed: the live directory may be the device-data one or the legacy roaming
# one, and `resource_dir` decides by which currently holds usable files.
Write-Host (Get-CheckpointText "row_01")
Push-Location $root
try {
    $resolvedJson = & cargo run --quiet -p sakatsu-desktop --bin resolve-resources
    if ($LASTEXITCODE -ne 0) { throw "resolve-resources failed with exit code $LASTEXITCODE" }
} finally {
    Pop-Location
}
$resolved = $resolvedJson | ConvertFrom-Json

# --- A unique scratch profile ------------------------------------------------------------------
$marker = 'checkpoint-profile.json'
if ($Reuse) {
    # A leaf name, resolved under the temporary directory, and checked to be a *marked checkpoint
    # profile* before anything else happens. This value reaches Remove-Item -Recurse further down:
    # an unchecked path here is a recursive delete of whatever somebody typed.
    if ($Reuse -ne (Split-Path -Leaf $Reuse)) {
        throw "-Reuse takes the profile's name, not a path: $Reuse"
    }
    $profileRoot = Join-Path ([System.IO.Path]::GetTempPath()) $Reuse
    $markerPath = Join-Path $profileRoot $marker
    if (-not (Test-Path $markerPath)) {
        throw "$profileRoot is not a checkpoint profile (no $marker). Refusing to touch it."
    }
    $recorded = Get-Content -Raw $markerPath | ConvertFrom-Json

    # **Reuse validates and launches; it does not prepare.** Running the scenario switch again
    # recreated the recovery fixtures, replanted the interrupted install and copied resources back
    # -- destroying exactly the state a restart row exists to check. Whatever this profile was
    # prepared for is what it stays prepared for.
    if ($PSBoundParameters.ContainsKey('Scenario') -and $Scenario -ne $recorded.scenario) {
        throw "This profile was prepared for -Scenario $($recorded.scenario). Reusing it as $Scenario would mean reseeding it, which is what a restart row exists to avoid."
    }
    $Scenario = $recorded.scenario
    if ($PSBoundParameters.ContainsKey('Store') -and $Store -ne $recorded.store) {
        throw "This profile was prepared with -Store $($recorded.store)."
    }
    $Store = $recorded.store
    if ($PSBoundParameters.ContainsKey('Analyzer') -and $Analyzer -ne $recorded.analyzer) {
        throw "This profile was prepared with analyzer '$($recorded.analyzer)'. A different engine segments differently, so the second launch would not be looking at the same thing."
    }
    $Analyzer = $recorded.analyzer
    $analyzerArg = if ($Analyzer) { " -Analyzer $Analyzer" } else { '' }
    $scenarioArg = if ($Scenario -ne 'reading') { " -Scenario $Scenario" } else { '' }
    Write-Host "Reusing $profileRoot (scenario $Scenario, store $Store) -- not reseeding it"
} else {
    # Timestamped, so two checkpoints never collide and a kept profile is never silently
    # overwritten by the next run.
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $profileRoot = Join-Path ([System.IO.Path]::GetTempPath()) "sakatsu-checkpoint-$Store-$stamp"
    New-Item -ItemType Directory -Force -Path $profileRoot | Out-Null
}

$scratchRoaming = Join-Path $profileRoot 'Roaming'
$scratchLocal = Join-Path $profileRoot 'Local'
$scratchDevice = Join-Path $scratchLocal 'com.sakatsu.app'
$scratchIdentity = Join-Path $scratchRoaming 'com.sakatsu.app'
New-Item -ItemType Directory -Force -Path $scratchIdentity, $scratchDevice | Out-Null

# --- Copy the resources in, so nothing the app writes can reach the real ones -------------------
#
# Only when preparing. On reuse this would copy back anything a scenario deliberately moved -- the
# kanji table an interrupted install has in its backup, for one -- and the row would then be looking
# at a state nobody arranged.
if (-not $Reuse) {
foreach ($resource in @(
        @{ Name = 'dictionaries'; From = $resolved.dictionaries },
        @{ Name = 'kanji'; From = $resolved.kanji })) {
    $target = Join-Path $scratchDevice $resource.Name
    if (Test-Path $target) {
        Write-Host "  $($resource.Name): already in the profile"
        continue
    }
    if (-not (Test-Path $resource.From)) {
        Write-Warning "No $($resource.Name) at $($resource.From) -- the checklist rows that need it will not be observable."
        continue
    }
    Write-Host "  copying $($resource.Name) from $($resource.From)..."
    Copy-Item -Recurse -Force -Path $resource.From -Destination $target
}
}

# --- Plant what this scenario needs to look at ------------------------------------------------
#
# After the resources are copied, because two of the fixtures are built from a real import; and
# before the environment is redirected, because the fixture binary takes the profile as an argument
# and resolves nothing from the environment. Nothing it writes can leave $scratchDevice.
function Invoke-Fixture {
    param([Parameter(Mandatory)][string[]]$FixtureArgs)
    Push-Location $root
    try {
        & cargo run --quiet -p sakatsu-desktop --bin checkpoint-fixture -- @FixtureArgs
        if ($LASTEXITCODE -ne 0) { throw "checkpoint-fixture failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
}

$relocationTarget = Join-Path $profileRoot 'moved-resources'
if (-not $Reuse) {
switch ($Scenario) {
    'recovery' {
        Write-Host ''
        Write-Host 'Preparing the recovery fixtures...'
        Invoke-Fixture @('recovery', $scratchDevice)
    }
    'loading' {
        Write-Host ''
        Write-Host 'Preparing the interrupted install...'
        Invoke-Fixture @('loading', $scratchDevice)
    }
    'loading-failed' {
        Write-Host ''
        Write-Host 'Preparing the interrupted install, with a table that cannot load...'
        Invoke-Fixture @('loading', $scratchDevice, '--empty-kanji')
    }
    'relocation' {
        New-Item -ItemType Directory -Force -Path $relocationTarget | Out-Null
        Write-Host ''
        Write-Host "Move target prepared inside the profile: $relocationTarget"
    }
}

# What this profile is, so a later -Reuse can refuse to be something else.
@{
    scenario = $Scenario
    store    = $Store
    analyzer = if ($Analyzer) { $Analyzer } else { '' }
    prepared = (Get-Date -Format 'o')
} | ConvertTo-Json | Set-Content -Path (Join-Path $profileRoot $marker) -Encoding utf8
}

if ($Store -eq 'Unavailable') {
    # A file, not a directory. The only way to reach the degraded path: `Store::open` calls
    # `create_dir_all` first, so an absent directory is simply created.
    $storePath = Join-Path $scratchIdentity 'store'
    if (Test-Path $storePath -PathType Container) { Remove-Item -Recurse -Force $storePath }
    Set-Content -Path $storePath -Value 'not a directory' -NoNewline
}

# --- Redirect, remembering what to put back ----------------------------------------------------
#
# **Every override, not the two that were remembered.** `resource_dir` answers an environment
# variable before it looks anywhere else, and there is one per resource: leaving
# SAKATSU_WORDNET_DIR or SAKATSU_MODEL_DIR set in the caller's shell sends that resource outside the
# profile, which the loading and relocation rows would then be quietly wrong about.
$overrides = @(
    'SAKATSU_DICT_DIR', 'SAKATSU_KANJI_DIR', 'SAKATSU_WORDNET_DIR', 'SAKATSU_AUDIO_DIR',
    'SAKATSU_MODEL_DIR', 'SAKATSU_MT_DIR', 'SAKATSU_PADDLE_DIR', 'SAKATSU_VLM_DIR',
    'SAKATSU_ANALYZER_SENTENCES'
)
$saved = @{}
foreach ($name in @('APPDATA', 'LOCALAPPDATA', 'SAKATSU_DEVICE_DATA_DIR', 'SAKATSU_ANALYZER', 'SAKATSU_ANALYZER_FAULT', 'npm_config_cache') + $overrides) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name)
}

# npm keeps its cache under LOCALAPPDATA, which is about to move. `node_modules` is already
# installed so `npm run dev` needs nothing from it, but an empty cache is a re-download waiting to
# happen the first time it does -- and a checkpoint that pauses to fetch packages is one nobody
# runs twice. Pinned to the real cache, and restored with everything else.
$realCache = & npm config get cache 2>$null
if ($LASTEXITCODE -eq 0 -and $realCache) { $npmCache = $realCache.Trim() } else { $npmCache = $null }

Write-Host ''
Write-Host "  profile      $profileRoot"
Write-Host "  store        $Store"
Write-Host "  resources    copied into $scratchDevice"
if ($AnalyzerFault) {
    Write-Host "  fault        $AnalyzerFault (SAKATSU_ANALYZER_FAULT -- debug build only; the sidecar is not contacted)" -ForegroundColor Yellow
}
if ($Analyzer) {
    Write-Host "  analyzer     $Analyzer (pinned -- record this with the observation)"
} else {
    Write-Host "  analyzer     as shipped (auto), NOT pinned" -ForegroundColor Yellow
    Write-Host '               auto prefers Ichiran when it answers, and the colloquial'
    Write-Host (Get-CheckpointText "row_02")
    Write-Host '               Ichiran installed those rows fail for the wrong reason.'
    Write-Host '               Use -Analyzer lindera to check them.'
}
Write-Host ''
Write-Host '  PASTE THESE, one at a time:' -ForegroundColor Cyan
Write-Host (Get-CheckpointText "row_03")
Write-Host (Get-CheckpointText "row_04")
Write-Host (Get-CheckpointText "row_05")
Write-Host ''
Write-Host (Get-CheckpointText "row_06") -ForegroundColor Cyan
Write-Host '  an unobserved row is not a passing row.' -ForegroundColor Cyan
Write-Host ''
Write-Host '    1. Ruby            sits over the characters it belongs to; nothing clipped'
Write-Host (Get-CheckpointText "row_07")
Write-Host (Get-CheckpointText "row_08")
Write-Host '    4. Mark a word     the status colour changes'
if ($Store -eq 'Empty') {
    Write-Host (Get-CheckpointText "row_09")
    Write-Host ''
    Write-Host (Get-CheckpointText "row_10") -ForegroundColor Cyan
    Write-Host '  The toggle is "Mark uncertain readings" in the app menu, and it is OFF by default.'
    Write-Host (Get-CheckpointText "row_11")
    Write-Host ''
    Write-Host (Get-CheckpointText "row_12") -ForegroundColor Cyan
    Write-Host '  what to look for cannot report this row either way:'
    Write-Host (Get-CheckpointText "row_13")
    Write-Host '      not under the word itself. Marking the word would caveat the wrong thing:'
    Write-Host '      the word is on the page and is what it is; the READING is the guess.'
    Write-Host (Get-CheckpointText "row_14")
    Write-Host (Get-CheckpointText "row_15")
    Write-Host (Get-CheckpointText "row_16")
    Write-Host ''
    Write-Host (Get-CheckpointText "row_17")
    Write-Host (Get-CheckpointText "row_18")
    Write-Host '    6c. toggle OFF     the dotted rule disappears again'
    Write-Host '    6d. throughout     the READING itself never changes.'
    Write-Host '        The toggle is display-only and never reaches the analysis, so a reading'
    Write-Host (Get-CheckpointText "row_19")
    Write-Host ''
    Write-Host '  THEN close the app and relaunch against this same profile:' -ForegroundColor Cyan
    Write-Host "      pwsh -File scripts/desktop-checkpoint.ps1 -Reuse $(Split-Path -Leaf $profileRoot)$analyzerArg$scenarioArg"
    Write-Host (Get-CheckpointText "row_20")
} else {
    Write-Host (Get-CheckpointText "row_21")
    Write-Host '    6. Reading/lookup  still work; losing saving is not losing reading'
    Write-Host '    7. Marking a word  refused, and says why'
}
# --- What to look at, for the scenario that was prepared ---------------------------------------
$fixtureCommand = "cargo run -p sakatsu-desktop --bin checkpoint-fixture --"
switch ($Scenario) {
    'recovery' {
        Write-Host '  RECOVERY ROWS. The panel is "Unfinished installations".' -ForegroundColor Cyan
        Write-Host '  Each container has its OWN two words, printed by the fixture above. A shared'
        Write-Host '  word could not tell which container an action had actually reached.'
        Write-Host '    R1. Four entries       checkpoint-restore, checkpoint-keep, checkpoint-blocked'
        Write-Host '                           and checkpoint-stale, each offering BOTH Keep current'
        Write-Host '                           and Restore previous.'
        Write-Host '    R2. Restore previous   on checkpoint-restore. The entry goes. Then in Reference:'
        Write-Host '                             restorable   must answer NOW, with no restart'
        Write-Host '                             restoreheld  must stop answering'
        Write-Host '                             keepheld, blockheld and staleheld must all still'
        Write-Host '                             answer: restoring one container changes only that one.'
        Write-Host '    R3. Keep current       on checkpoint-keep. Before pressing, read the line under'
        Write-Host '                           the buttons: it must say the previous version stops being'
        Write-Host '                           protected and may be removed. After pressing, search'
        Write-Host '                           keepheld: it must still answer.'
        Write-Host '    R4. Failed restore     Restore previous on checkpoint-blocked. It must FAIL with'
        Write-Host '                           a reason naming the missing generation -- not "already'
        Write-Host '                           resolved", and worded for THIS action rather than for a'
        Write-Host '                           publisher finding an unresolved transaction.'
        Write-Host '                           The entry must still be there to try again.'
        Write-Host '        then, in another shell:'
        Write-Host "          $fixtureCommand unblock $scratchDevice"
        Write-Host '                           press Restore previous again: it must succeed, and'
        Write-Host '                           blockable must answer.'
        Write-Host '    R5. Stale action       checkpoint-stale has its OWN transaction so that R3 does'
        Write-Host '                           not consume it. With the panel still on screen, in'
        Write-Host '                           another shell:'
        Write-Host "          $fixtureCommand resolve $scratchDevice checkpoint-stale"
        Write-Host '                           then press either button on checkpoint-stale. It must say'
        Write-Host '                           it was already resolved elsewhere and that this is what'
        Write-Host '                           is true now, WITHOUT claiming the action succeeded and'
        Write-Host '                           WITHOUT error styling. The entry goes; the explanation'
        Write-Host '                           REMAINS beside the list until Dismiss is pressed.'
        Write-Host '    R6. Attribution        after R2 the credits footer must name the restored'
        Write-Host '                           generation, not the superseded one.'
        Write-Host '    R7. Storage path       Settings > Recognition: the stored-in path must show no'
        Write-Host '                           extended-length prefix.'
        Write-Host ''
    }
    'loading' {
        Write-Host '  RESTORED-RESOURCE LOADING.' -ForegroundColor Cyan
        Write-Host '    L1. kanji              "Restored, and in use now."'
        Write-Host '    L2. frequency-jpdb     restored, and says nothing else is needed -- it is not'
        Write-Host '                           the selected list, and restoring it must not select it.'
        Write-Host '    L3. wordnet            says whether this session picked it up was NOT'
        Write-Host '                           established. It must not say "restart".'
        Write-Host '    L4. Then relaunch with -Scenario loading-failed for the fourth sentence.'
        Write-Host ''
    }
    'loading-failed' {
        Write-Host '  RESTORED-RESOURCE LOADING: the failure.' -ForegroundColor Cyan
        Write-Host '    L5. kanji              restored on disk, and says loading it did not work,'
        Write-Host '                           with the reason ("produced no characters").'
        Write-Host '                           It must NOT say "in use now".'
        Write-Host ''
    }
    'relocation' {
        Write-Host '  MOVE AND DEFERRED CLEANUP.' -ForegroundColor Cyan
        Write-Host '    M1. Before moving      read a passage and look a word up, so there is a reader'
        Write-Host '                           in flight to keep working.'
        Write-Host '    M2. Move               Settings > storage, choose:'
        Write-Host "                             $relocationTarget"
        Write-Host '                           with "remove the old copies" ON. It reports how many'
        Write-Host '                           folders it verified.'
        Write-Host '    M3. During and after   the same lookup still answers. Nothing goes blank.'
        Write-Host '    M4. Afterwards         import or repair something, or open the setup panel:'
        Write-Host '                           the paths it shows are under the NEW location.'
        Write-Host '    M5. Change one old     in another shell, before restarting:'
        Write-Host "          $fixtureCommand touch-old $scratchDevice kanji"
        Write-Host '    M6. Restart            close the app and relaunch against this same profile:'
        Write-Host "          pwsh -File scripts/desktop-checkpoint.ps1 -Reuse $(Split-Path -Leaf $profileRoot)$analyzerArg$scenarioArg"
        Write-Host '                           the new location is still in use; the unchanged old'
        Write-Host '                           folders are gone; the one changed in M5 is still there,'
        Write-Host '                           and the log says it was kept because it changed.'
        Write-Host ''
    }
}

Write-Host ''

$exitCode = 0
try {
    $env:APPDATA = $scratchRoaming
    $env:LOCALAPPDATA = $scratchLocal
    # **Set for every scenario except relocation, and that exception is the point.**
    #
    # `device_data_dir()` answers this variable *before* the locator a move writes, so a relocation
    # launch that sets it would go on reading the original directory however the move went -- and
    # the restart row could not observe that relocation persisted at all. Left unset, the chain is
    # the locator under the isolated APPDATA, then the isolated LOCALAPPDATA: both inside the
    # profile, which the check below asserts rather than assumes.
    if ($Scenario -ne 'relocation') {
        $env:SAKATSU_DEVICE_DATA_DIR = $scratchDevice
    } else {
        Remove-Item Env:SAKATSU_DEVICE_DATA_DIR -ErrorAction SilentlyContinue
    }
    if ($npmCache) { $env:npm_config_cache = $npmCache }
    # Set only when asked. Clearing it otherwise would be a decision of its own: an override left
    # in the caller's shell is exactly the sort of thing that makes a checkpoint unreproducible,
    # but so is silently overriding what the reader chose. Unset means "as shipped", which the
    # banner has already said out loud.
    if ($Analyzer) { $env:SAKATSU_ANALYZER = $Analyzer }
    # Only when asked, and cleared otherwise: a fault left in the caller's shell must never make an
    # ordinary checkpoint run look broken.
    if ($AnalyzerFault) {
        $env:SAKATSU_ANALYZER_FAULT = $AnalyzerFault
    } else {
        Remove-Item Env:SAKATSU_ANALYZER_FAULT -ErrorAction SilentlyContinue
    }
    # Cleared rather than pointed anywhere: the device-data location already puts the copies where
    # `resource_dir` looks, and a stale override in the caller's shell would silently defeat the
    # whole isolation.
    foreach ($name in $overrides) { Remove-Item "Env:$name" -ErrorAction SilentlyContinue }

    # --- Assert the isolation rather than trusting it ------------------------------------------
    #
    # Asked of the app, under the environment it is about to run in, and asked about **every**
    # resource -- the launcher used to check four, so an escaped WordNet, audio or model path was
    # invisible to it. Containment is decided in `resolve-resources`, by path component and with
    # junctions resolved: `profile-other` is not inside `profile`, and a junction planted inside the
    # profile can point anywhere.
    Push-Location $root
    try {
        & cargo run --quiet -p sakatsu-desktop --bin resolve-resources -- --inside $profileRoot
        if ($LASTEXITCODE -ne 0) {
            throw "Refusing to launch: a resource resolves outside $profileRoot, so this row would be looking at real files."
        }
    } finally {
        Pop-Location
    }
    Write-Host '  isolation    every resolved resource path is inside the profile' -ForegroundColor Green
    Write-Host ''

    # The interface as it stands before the launch. Taken again afterwards: `tauri dev` serves the
    # UI from source and reloads on a save, so a digest from before the observation is not
    # necessarily what was on screen at the end of it.
    Push-Location $root
    try {
        $uiBefore = & cargo run --quiet -p sakatsu-desktop --bin checkpoint-fixture -- identity $root --ui-only
    } finally {
        Pop-Location
    }
    Write-Host '  interface identity, before launching (source):' -ForegroundColor Cyan
    $uiBefore | ForEach-Object { Write-Host "    $_" }
    Write-Host ''

    if ($PrepareOnly) {
        Write-Host 'Prepared, not launched (-PrepareOnly).' -ForegroundColor Cyan
        Write-Host '  The executable identity below is PRELIMINARY: nothing has been rebuilt or run.' -ForegroundColor Yellow
        Push-Location $root
        try {
            & cargo run --quiet -p sakatsu-desktop --bin checkpoint-fixture -- identity $root |
                ForEach-Object { Write-Host "    $_" }
        } finally {
            Pop-Location
        }
        Write-Host "  launch:  pwsh -File scripts/desktop-checkpoint.ps1 -Reuse $(Split-Path -Leaf $profileRoot)$analyzerArg$scenarioArg"
    } else {
        # --- Identify the binary that actually runs -------------------------------------------
        #
        # **Not the one on disk beforehand.** `cargo tauri dev` rebuilds, so a hash taken before it
        # can name yesterday's executable. This waits for the window's own process and hashes the
        # image it is running from, which is the only file that can be called the observed build.
        $identityFile = Join-Path $profileRoot 'observed-identity.json'
        Remove-Item $identityFile -ErrorAction SilentlyContinue
        $watcher = Start-Job -ScriptBlock {
            param($into)
            $deadline = (Get-Date).AddMinutes(10)
            while ((Get-Date) -lt $deadline) {
                $app = Get-Process -Name 'sakatsu-desktop' -ErrorAction SilentlyContinue |
                    Select-Object -First 1
                if ($app -and $app.Path) {
                    @{
                        pid       = $app.Id
                        path      = $app.Path
                        started   = $app.StartTime.ToString('o')
                        sha256    = (Get-FileHash -Algorithm SHA256 -Path $app.Path).Hash.ToLower()
                        bytes     = (Get-Item $app.Path).Length
                        recorded  = (Get-Date).ToString('o')
                    } | ConvertTo-Json | Set-Content -Path $into -Encoding utf8
                    return
                }
                Start-Sleep -Milliseconds 250
            }
        } -ArgumentList $identityFile

        Push-Location $root
        try {
            cargo tauri dev
            $exitCode = $LASTEXITCODE
        } finally {
            Pop-Location
        }
        Stop-Job $watcher -ErrorAction SilentlyContinue
        Remove-Job $watcher -Force -ErrorAction SilentlyContinue

        Write-Host ''
        Write-Host 'OBSERVED BUILD IDENTITY -- record this with the rows:' -ForegroundColor Cyan
        if (Test-Path $identityFile) {
            $observed = Get-Content -Raw $identityFile | ConvertFrom-Json
            Write-Host "    executable           $($observed.sha256)"
            Write-Host "    image                $($observed.path) ($($observed.bytes) bytes)"
            Write-Host "    process              pid $($observed.pid), started $($observed.started)"
        } else {
            Write-Host '    the app process was never seen -- nothing was observed, so record nothing' -ForegroundColor Yellow
        }

        Push-Location $root
        try {
            $uiAfter = & cargo run --quiet -p sakatsu-desktop --bin checkpoint-fixture -- identity $root --ui-only
        } finally {
            Pop-Location
        }
        $uiAfter | ForEach-Object { Write-Host "    $_" }
        if (($uiBefore -join "`n") -ne ($uiAfter -join "`n")) {
            Write-Host ''
            Write-Host '    THE INTERFACE SOURCE CHANGED DURING THIS OBSERVATION.' -ForegroundColor Yellow
            Write-Host '    tauri dev hot-reloads on a save, so what you saw may not be one build.' -ForegroundColor Yellow
            Write-Host '    Record both digests, or re-run the rows against a tree nobody is editing.' -ForegroundColor Yellow
        }
        Write-Host ''
    }
} finally {
    foreach ($name in $saved.Keys) {
        if ($null -eq $saved[$name]) {
            Remove-Item "Env:$name" -ErrorAction SilentlyContinue
        } else {
            Set-Item "Env:$name" $saved[$name]
        }
    }

    if ($Clean) {
        Remove-Item -Recurse -Force $profileRoot -ErrorAction SilentlyContinue
        Write-Host "removed $profileRoot"
    } else {
        Write-Host ''
        Write-Host "profile kept: $profileRoot" -ForegroundColor Cyan
        Write-Host "  reuse:  pwsh -File scripts/desktop-checkpoint.ps1 -Reuse $(Split-Path -Leaf $profileRoot)$analyzerArg$scenarioArg"
        Write-Host "  remove: Remove-Item -Recurse -Force '$profileRoot'"
    }
}

# The launch's own result, not the cleanup's: a checkpoint that failed to start must not look
# like one that started and was closed.
exit $exitCode
