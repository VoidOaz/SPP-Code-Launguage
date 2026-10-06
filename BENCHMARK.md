# SPP 2.0 Benchmark

The reference benchmark is a simple integer loop:

```text
for i = 0..4,999,999:
    sum += (i % 97) * 3
```

SPP 2.0 should report `optimized bytecode VM` for `examples/bench.spp`.

Run:

```powershell
spp benchmark examples\bench.spp 5
```

The C++ baseline remains available as `benchmark.cpp`:

```powershell
g++ -O3 benchmark.cpp -o benchmark.exe
.\benchmark.exe
```

Do not publish a universal language-speed claim from this one loop. Pair every result with the exact CPU, OS, compiler version/profile, and benchmark run count.
