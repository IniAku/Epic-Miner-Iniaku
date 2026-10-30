// Batched Cuckatoo19 lean trimmer (epic.iniaku.my.id).
//
// Trimmer lama memproses satu graf per panggilan: C19 hanya 2048 work-item & ~770 perintah
// OpenCL per graf, sehingga GPU (RTX 4070) terpakai ~1% dan ~120 gps. Di sini `batch` graf
// diproses bersama: satu launch kernel per ronde utk seluruh batch (batch*2048 work-item),
// jadi jumlah perintah per graf turun dari ~770 ke ~1.

use ocl;
use ocl::{Buffer, Context, Kernel, Program, Queue, SpatialDims};

use trimmer::{find_device, find_platform};

const LOCAL_WORK_SIZE: usize = 256;
/// Edge hidup maksimum per graf yg diambil sesudah trimming (sisanya diabaikan).
pub const MAX_EDGES: usize = 2048;
/// u32 per graf di buffer hasil: [_, count, _, _] lalu MAX_EDGES x [u, v, nonce, _].
pub const AUX_STRIDE: usize = 4 + 4 * MAX_EDGES;

const MODE_SETCNT: u32 = 1;
const MODE_TRIM: u32 = 2;
const MODE_EXTRACT: u32 = 3;

pub struct BatchTrimmer {
	pub batch: usize,
	pub trims: u32,
	q: Queue,
	keys: Buffer<u64>,
	edges: Buffer<u32>,
	counters: Buffer<u32>,
	aux: Buffer<u32>,
	kernel: Kernel,
	items: usize,
	pub device_name: String,
	pub device_id: usize,
}

impl BatchTrimmer {
	pub fn build(
		platform_name: Option<&str>,
		device_id: Option<usize>,
		edge_bits: u8,
		batch: usize,
		trims: u32,
	) -> ocl::Result<BatchTrimmer> {
		let platform = find_platform(platform_name)
			.ok_or::<ocl::Error>("Can't find OpenCL platform".into())?;
		let device = find_device(&platform, device_id)?;
		let context = Context::builder().platform(platform).devices(device).build()?;
		let q = Queue::new(&context, device, None)?;
		let program = Program::builder()
			.devices(device)
			.src(SRC)
			.cmplr_def("EDGEBITS", edge_bits as i32)
			.cmplr_def("MAXE", MAX_EDGES as i32)
			.build(&context)?;

		let words = 1usize << (edge_bits - 5); // bit per edge / node, per graf
		let items = 1usize << (edge_bits - 8); // 256 edge per work-item
		let keys = Buffer::<u64>::builder().queue(q.clone()).len(batch * 4).build()?;
		let edges = Buffer::<u32>::builder().queue(q.clone()).len(batch * words).build()?;
		let counters = Buffer::<u32>::builder().queue(q.clone()).len(batch * words).build()?;
		let aux = Buffer::<u32>::builder().queue(q.clone()).len(batch * AUX_STRIDE).build()?;

		let kernel = Kernel::builder()
			.name("LeanBatch")
			.program(&program)
			.queue(q.clone())
			.global_work_size(batch * items)
			.local_work_size(SpatialDims::One(LOCAL_WORK_SIZE))
			.arg(&keys)
			.arg(&edges)
			.arg(&counters)
			.arg(&aux)
			.arg(MODE_SETCNT)
			.arg(0u32)
			.build()?;

		Ok(BatchTrimmer {
			batch,
			trims,
			q,
			keys,
			edges,
			counters,
			aux,
			kernel,
			items,
			device_name: device.name()?,
			device_id: device_id.unwrap_or(0),
		})
	}

	/// Trim `self.batch` graf (siphash key per graf) & kembalikan buffer hasil:
	/// graf g mulai di g*AUX_STRIDE, format yg sama dgn input finder::Graph::search.
	pub fn run(&self, keys: &[[u64; 4]]) -> ocl::Result<Vec<u32>> {
		assert_eq!(keys.len(), self.batch);
		let flat: Vec<u64> = keys.iter().flat_map(|k| k.iter().cloned()).collect();
		self.keys.write(&flat).enq()?;
		self.edges.cmd().fill(0xFFFF_FFFF, None).enq()?;
		self.aux.cmd().fill(0, None).enq()?;

		for l in 0..self.trims {
			self.counters.cmd().fill(0, None).enq()?;
			self.kernel.set_arg(4, MODE_SETCNT)?;
			self.kernel.set_arg(5, l & 1)?;
			unsafe {
				self.kernel.enq()?;
			}
			let mode = if l == self.trims - 1 { MODE_EXTRACT } else { MODE_TRIM };
			self.kernel.set_arg(4, mode)?;
			unsafe {
				self.kernel.enq()?;
			}
		}

		let mut out = vec![0u32; self.batch * AUX_STRIDE];
		self.aux.read(&mut out).enq()?;
		self.q.finish()?;
		let _ = self.items;
		Ok(out)
	}
}

const SRC: &str = r#"
typedef uint u32;
typedef ulong u64;

#define NEDGES ((u64)1 << EDGEBITS)
#define EDGEMASK (NEDGES - 1)
// work-item per graf (256 edge masing-masing) & u32 per bitmap graf
#define ITEMS (NEDGES / 256)
#define WORDS (NEDGES / 32)
#define STRIDE (4 + 4 * MAXE)

#define MODE_SETCNT 1
#define MODE_TRIM 2
#define MODE_EXTRACT 3

#define SIPROUND \
  do { \
    v0 += v1; v2 += v3; v1 = rotate(v1,(ulong)13); \
    v3 = rotate(v3,(ulong)16); v1 ^= v0; v3 ^= v2; \
    v0 = rotate(v0,(ulong)32); v2 += v1; v0 += v3; \
    v1 = rotate(v1,(ulong)17);   v3 = rotate(v3,(ulong)21); \
    v1 ^= v2; v3 ^= v0; v2 = rotate(v2,(ulong)32); \
  } while(0)

u32 dipnode(ulong v0i, ulong v1i, ulong v2i, ulong v3i, u64 nce, uint uorv) {
	ulong nonce = 2 * nce + uorv;
	ulong v0 = v0i, v1 = v1i, v2 = v2i, v3 = v3i ^ nonce;
	SIPROUND; SIPROUND;
	v0 ^= nonce;
	v2 ^= 0xff;
	SIPROUND; SIPROUND; SIPROUND; SIPROUND;
	return (u32)((v0 ^ v1 ^ v2 ^ v3) & EDGEMASK);
}

// Satu work-item = 256 edge dari satu graf; graf = gid / ITEMS.
// edges: bit 1 = edge hidup (urutan bit MSB dulu, sama dgn trimmer lama).
// counters: bit node yg punya edge hidup di sisi uorv (SETCNT), dibaca saat TRIM/EXTRACT.
__kernel void LeanBatch(__global const ulong *keys, __global uint8 *edges,
                        __global uint *counters, __global u32 *aux,
                        const u32 mode, const u32 uorv)
{
	const u32 gid = get_global_id(0);
	const u32 g = gid / ITEMS;
	const u32 item = gid % ITEMS;

	const ulong v0i = keys[4 * g], v1i = keys[4 * g + 1], v2i = keys[4 * g + 2], v3i = keys[4 * g + 3];
	__global uint8 *E = edges + (ulong)g * ITEMS;
	__global uint *C = counters + (ulong)g * WORDS;
	__global u32 *A = aux + (ulong)g * STRIDE;

	const u64 start = (u64)item * 256;
	uint8 load = E[item];
	u32 el[8] = { load.s0, load.s1, load.s2, load.s3, load.s4, load.s5, load.s6, load.s7 };
	if (mode == MODE_SETCNT && !(el[0] | el[1] | el[2] | el[3] | el[4] | el[5] | el[6] | el[7]))
		return;

	for (int i = 0; i < 8; i++) {
		u32 ee = el[i];
		while (ee) {
			const u32 pos = clz(ee);
			const u64 nonce = start + i * 32 + pos;
			u32 hash = dipnode(v0i, v1i, v2i, v3i, nonce, uorv);
			if (mode == MODE_SETCNT) {
				atomic_or(&C[hash / 32], (u32)1 << (hash % 32));
			} else {
				hash ^= 1;
				const bool lives = (C[hash / 32] >> (hash % 32)) & 1;
				if (!lives) {
					el[i] ^= 0x80000000u >> pos;
				} else if (mode == MODE_EXTRACT) {
					const u32 p = atomic_inc(&A[1]);
					if (p < MAXE) {
						const u32 idx = 4 + p * 4;
						A[idx] = dipnode(v0i, v1i, v2i, v3i, nonce, 0);
						A[idx + 1] = dipnode(v0i, v1i, v2i, v3i, nonce, 1);
						A[idx + 2] = (u32)nonce;
						A[idx + 3] = 0;
					}
				}
			}
			ee ^= 0x80000000u >> pos;
		}
	}
	if (mode == MODE_TRIM)
		E[item] = (uint8)(el[0], el[1], el[2], el[3], el[4], el[5], el[6], el[7]);
}
"#;
