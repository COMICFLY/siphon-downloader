# Siphon External CLI/JSON Hook Example
# Reads a JSON request from stdin and writes a JSON response to stdout.
$inputJson = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($inputJson)) {
    exit 0
}

$req = $inputJson | ConvertFrom-Json

if ($req.hook -eq "resolve") {
    $resp = @{
        ok = $true
        url = $req.url
        headers = @{
            "X-Siphon-Plugin" = "example-hook"
        }
        filename = $null
        extra_tasks = @()
        note = "Resolved by example-hook"
    }
    $resp | ConvertTo-Json -Compress -Depth 5
} elseif ($req.hook -eq "post_download") {
    $resp = @{
        ok = $true
        note = "Post-processed file: $($req.file_path)"
    }
    $resp | ConvertTo-Json -Compress -Depth 5
}
