#!/bin/bash
#SBATCH --job-name=test-gpu-eff
#SBATCH --partition=test
#SBATCH --account=rcc-staff
#SBATCH --nodelist=midway3-0298
#SBATCH --nodes=1
#SBATCH --ntasks=1
#SBATCH --cpus-per-task=4
#SBATCH --gres=gpu:1
#SBATCH --time=00:20:00
#SBATCH --output=gpu-eff-test-%j.out

echo "=== Job $SLURM_JOB_ID on $(hostname) ==="
echo "=== CUDA_VISIBLE_DEVICES: $CUDA_VISIBLE_DEVICES ==="
echo "=== Start: $(date) ==="
echo ""

module load python 2>/dev/null || module load anaconda3 2>/dev/null || true

python3 - <<'PYEOF'
import time
import subprocess
import sys

def poll_gpu_util():
    try:
        out = subprocess.check_output(
            ["nvidia-smi", "--query-gpu=utilization.gpu,memory.used,memory.total",
             "--format=csv,noheader,nounits"],
            text=True
        ).strip()
        return out
    except Exception:
        return "unavailable"

try:
    import torch
    if not torch.cuda.is_available():
        print("ERROR: CUDA not available to PyTorch", flush=True)
        sys.exit(1)
    print(f"PyTorch {torch.__version__}, GPU: {torch.cuda.get_device_name(0)}", flush=True)
except ImportError:
    print("ERROR: PyTorch not available — load the correct module before submitting", flush=True)
    sys.exit(1)

import torch

device = torch.device("cuda:0")
# Large matrix to saturate the V100
N = 8192

# --- Phase 1: 100% utilization for 5 minutes ---
print("\n=== Phase 1: 100% GPU utilization (5 min) ===", flush=True)
a = torch.randn(N, N, device=device, dtype=torch.float32)
b = torch.randn(N, N, device=device, dtype=torch.float32)

phase1_end = time.time() + 300  # 5 minutes
samples = 0
while time.time() < phase1_end:
    c = torch.mm(a, b)
    torch.cuda.synchronize()
    samples += 1
    if samples % 50 == 0:
        print(f"  [{time.strftime('%H:%M:%S')}] util: {poll_gpu_util()}", flush=True)

print(f"Phase 1 done — {samples} matmul iterations", flush=True)

# --- Phase 2: ~50% utilization (5s compute, 5s idle) for 5 minutes ---
print("\n=== Phase 2: ~50% GPU utilization (5 min, 5s on / 5s off) ===", flush=True)
phase2_end = time.time() + 300
cycles = 0
while time.time() < phase2_end:
    burst_end = min(time.time() + 5, phase2_end)
    while time.time() < burst_end:
        c = torch.mm(a, b)
        torch.cuda.synchronize()
    print(f"  [{time.strftime('%H:%M:%S')}] util during burst: {poll_gpu_util()}", flush=True)
    time.sleep(5)
    cycles += 1

print(f"Phase 2 done — {cycles} cycles", flush=True)

del a, b, c
torch.cuda.empty_cache()
PYEOF

echo ""
echo "=== End: $(date) ==="
echo ""
echo "=== Check efficiency with: ==="
echo "    slurm-report -j $SLURM_JOB_ID"
echo "    (wait ~2 min after job ends for sacct to flush)"
