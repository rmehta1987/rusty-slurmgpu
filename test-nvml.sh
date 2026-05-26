#!/bin/bash
#SBATCH --job-name=test-nvml
#SBATCH --partition=test
#SBATCH --account=rcc-staff
#SBATCH --nodelist=midway3-0298
#SBATCH --nodes=1
#SBATCH --ntasks=1
#SBATCH --cpus-per-task=1
#SBATCH --gres=gpu:1
#SBATCH --time=00:05:00
#SBATCH --output=nvml-test-%j.out

echo "=== Node: $(hostname) ==="
echo "=== GRES allocated: $SLURM_JOB_GRES ==="
echo "=== CUDA_VISIBLE_DEVICES: $CUDA_VISIBLE_DEVICES ==="
echo ""

echo "=== nvidia-smi ==="
nvidia-smi

echo ""
echo "=== /dev/nvidia* devices ==="
ls -la /dev/nvidia*

echo ""
echo "=== scontrol show job ==="
scontrol show job $SLURM_JOB_ID | grep -E "GRES|Tres|NodeList"
