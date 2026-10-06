import time

N = 5_000_000
runs = 5
samples = []

for _ in range(runs):
    start = time.perf_counter()
    total = 0
    for i in range(N):
        total += (i % 97) * 3
    samples.append(time.perf_counter() - start)

print(f"Python avg: {sum(samples)/runs*1000:.3f} ms")
print(f"Python min: {min(samples)*1000:.3f} ms")
print(f"Python max: {max(samples)*1000:.3f} ms")
print(f"checksum: {total}")
