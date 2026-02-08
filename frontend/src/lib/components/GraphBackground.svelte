<script>
	let canvas = $state(null);
	let animId = $state(null);

	const NODE_COUNT = 70;
	const MAX_DIST = 160;
	const NODE_RADIUS = 2.5;
	const BASE_SPEED = 0.25;

	const PINK = { r: 232, g: 101, b: 122 };
	const GREEN = { r: 163, g: 184, b: 108 };

	function randomBetween(a, b) {
		return a + Math.random() * (b - a);
	}

	function createNodes(w, h) {
		const nodes = [];
		for (let i = 0; i < NODE_COUNT; i++) {
			const isPink = Math.random() < 0.6;
			const color = isPink ? PINK : GREEN;
			nodes.push({
				x: Math.random() * w,
				y: Math.random() * h,
				vx: randomBetween(-BASE_SPEED, BASE_SPEED),
				vy: randomBetween(-BASE_SPEED, BASE_SPEED),
				r: color.r,
				g: color.g,
				b: color.b,
				radius: randomBetween(1.5, 3.5),
			});
		}
		return nodes;
	}

	$effect(() => {
		if (!canvas) return;

		const ctx = canvas.getContext('2d');
		let w, h;
		let nodes;

		function resize() {
			const rect = canvas.parentElement.getBoundingClientRect();
			const dpr = window.devicePixelRatio || 1;
			w = rect.width;
			h = rect.height;
			canvas.width = w * dpr;
			canvas.height = h * dpr;
			canvas.style.width = w + 'px';
			canvas.style.height = h + 'px';
			ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		}

		resize();
		nodes = createNodes(w, h);

		function tick() {
			ctx.clearRect(0, 0, w, h);

			// move nodes
			for (const n of nodes) {
				n.x += n.vx;
				n.y += n.vy;

				// wrap around
				if (n.x < -MAX_DIST) n.x = w + MAX_DIST;
				if (n.x > w + MAX_DIST) n.x = -MAX_DIST;
				if (n.y < -MAX_DIST) n.y = h + MAX_DIST;
				if (n.y > h + MAX_DIST) n.y = -MAX_DIST;
			}

			// draw edges
			for (let i = 0; i < nodes.length; i++) {
				for (let j = i + 1; j < nodes.length; j++) {
					const a = nodes[i];
					const b = nodes[j];
					const dx = a.x - b.x;
					const dy = a.y - b.y;
					const dist = Math.sqrt(dx * dx + dy * dy);
					if (dist < MAX_DIST) {
						const opacity = 0.18 * (1 - dist / MAX_DIST);
						ctx.beginPath();
						ctx.moveTo(a.x, a.y);
						ctx.lineTo(b.x, b.y);
						ctx.strokeStyle = `rgba(255, 255, 255, ${opacity})`;
						ctx.lineWidth = 0.8;
						ctx.stroke();
					}
				}
			}

			// draw nodes
			for (const n of nodes) {
				ctx.beginPath();
				ctx.arc(n.x, n.y, n.radius, 0, Math.PI * 2);
				ctx.fillStyle = `rgba(${n.r}, ${n.g}, ${n.b}, 0.6)`;
				ctx.fill();
			}

			animId = requestAnimationFrame(tick);
		}

		tick();

		const onResize = () => {
			resize();
			// re-clamp nodes to new bounds
			for (const n of nodes) {
				if (n.x > w + MAX_DIST) n.x = Math.random() * w;
				if (n.y > h + MAX_DIST) n.y = Math.random() * h;
			}
		};

		window.addEventListener('resize', onResize);

		return () => {
			if (animId) cancelAnimationFrame(animId);
			window.removeEventListener('resize', onResize);
		};
	});
</script>

<canvas bind:this={canvas} class="graph-bg" aria-hidden="true"></canvas>

<style>
	.graph-bg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
</style>
