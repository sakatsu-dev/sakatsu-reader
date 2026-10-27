# Serve an OCR vision-language model with vLLM, for measurement against the corpus.
#
#   pwsh -File scripts/serve-vlm.ps1 paddle-vl        # the general model
#   pwsh -File scripts/serve-vlm.ps1 paddle-vl-manga  # the manga fine-tune
#   pwsh -File scripts/serve-vlm.ps1 dots-ocr -Port 8001
#   pwsh -File scripts/serve-vlm.ps1 -List
#
# Then, in another shell, score it:
#
#   $env:SAKATSU_VLM_ENDPOINT = "http://127.0.0.1:8000"
#   $env:SAKATSU_VLM_LABEL    = "paddle-vl-manga"
#   cargo test -p sakatsu-core --test ocr_corpus -- --ignored --nocapture
#
# vLLM serves ONE model per process, so comparing candidates means running this once per
# model and scoring in between. `SAKATSU_VLM_LABEL` is what keeps the rows apart afterwards.

[CmdletBinding()]
param(
    [Parameter(Position = 0)] [string] $Candidate,
    [int] $Port = 8000,
    # A 0.9B model needs a fraction of a 16 GB card. The default leaves room for the
    # desktop, which is also using the GPU.
    [double] $GpuFraction = 0.5,
    [switch] $List
)

$ErrorActionPreference = 'Stop'

# Every one of these has been checked against the registry in the installed vLLM — the
# architecture string in the model's config.json must appear in
# vllm/model_executor/models/registry.py or the server dies at load with an unhelpful error.
$candidates = [ordered]@{
    'paddle-vl'       = @{ Repo = 'PaddlePaddle/PaddleOCR-VL-1.6';      Arch = 'PaddleOCRVLForConditionalGeneration'; Note = 'general document VLM; measured 0.1-0.7 s/crop on a 4070 Ti SUPER' }
    'paddle-vl-manga' = @{ Repo = 'jzhang533/PaddleOCR-VL-For-Manga';   Arch = 'PaddleOCRVLForConditionalGeneration'; Note = 'manga fine-tune; BEST MEASURED — and needs no vLLM, see below' }
    'dots-ocr'        = @{ Repo = 'rednote-hilab/dots.ocr';             Arch = 'DotsOCRForCausalLM';                  Note = 'MIT, 1.7B, layout + reading order in one model' }
    'qwen3-vl'        = @{ Repo = 'Qwen/Qwen3-VL-2B-Instruct';          Arch = 'Qwen3VLForConditionalGeneration';     Note = 'general VLM, not an OCR specialist - a control, not a candidate' }
}

if ($List -or -not $Candidate) {
    Write-Host "Neither PaddleOCR-VL model needs this script.`n" -ForegroundColor Yellow
    Write-Host "  A GGUF build of each is published, and Ollama pulls the projector with it,"
    Write-Host "  so there is no vLLM, WSL, Python or CUDA toolkit in the way:`n"
    Write-Host "    ollama pull hf.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF" -ForegroundColor Cyan
    Write-Host "    `$env:SAKATSU_VLM_ENDPOINT = `"http://127.0.0.1:11434`"" -ForegroundColor Cyan
    Write-Host "    `$env:SAKATSU_VLM_MODEL    = `"hf.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF:latest`"`n" -ForegroundColor Cyan
    Write-Host "  Measured that way over the 141-capture corpus: 0.0297 mean character error"
    Write-Host "  against 0.0975 for Auto, at 0.1 s per crop on a 4070 Ti SUPER. Settings >"
    Write-Host "  Recognition can install the same thing without a terminal.`n"
    Write-Host "  This script stays for the candidates below that have no GGUF.`n" -ForegroundColor DarkGray

    Write-Host "Candidates:`n"
    foreach ($key in $candidates.Keys) {
        $c = $candidates[$key]
        Write-Host ("  {0,-16} {1}" -f $key, $c.Repo) -ForegroundColor Cyan
        Write-Host ("  {0,-16} {1}`n" -f '', $c.Note)
    }
    Write-Host "Usage: pwsh -File scripts/serve-vlm.ps1 <name> [-Port 8000] [-GpuFraction 0.5]"
    return
}

if (-not $candidates.Contains($Candidate)) {
    throw "Unknown candidate '$Candidate'. Run with -List to see them."
}
$repo = $candidates[$Candidate].Repo

# Both variables are WSL2-specific and neither failure names WSL as the cause:
#
#  - Pinned memory is supported on WSL2 kernels >= 4.19.121 but OFF by default, and vLLM's
#    V2 model runner needs UVA, which needs it. Without this the engine exits at startup
#    with "RuntimeError: UVA is not available".
#  - FlashInfer's sampler JIT-compiles a CUDA kernel and wants nvcc, which the pip wheels do
#    not ship. Disabling it costs nothing: the client pins temperature to 0, so the top-k /
#    top-p path it accelerates is never taken.
$env_prefix = 'VLLM_WSL2_ENABLE_PIN_MEMORY=1 VLLM_USE_FLASHINFER_SAMPLER=0'
$serve = "$env_prefix ~/vllm-env/bin/vllm serve $repo --port $Port --gpu-memory-utilization $GpuFraction"

Write-Host "$Candidate  ->  $repo" -ForegroundColor Cyan
Write-Host "  $($candidates[$Candidate].Note)"
Write-Host "`nFirst run downloads the weights. Ready when it logs 'Application startup complete'.`n"
Write-Host "Score it with:" -ForegroundColor Green
Write-Host "  `$env:SAKATSU_VLM_ENDPOINT = `"http://127.0.0.1:$Port`""
Write-Host "  `$env:SAKATSU_VLM_MODEL    = `"$repo`""
Write-Host "  `$env:SAKATSU_VLM_LABEL    = `"$Candidate`""
Write-Host "  cargo test -p sakatsu-core --test ocr_corpus -- --ignored --nocapture`n"

wsl -e bash -lc "cd ~ && $serve"
