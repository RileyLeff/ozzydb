<script>
	let { topColor = 'var(--black)', bottomColor = 'var(--bg)' } = $props();
</script>

<div class="wavy-divider">
	<svg
		xmlns="http://www.w3.org/2000/svg"
		viewBox="0 0 1440 70"
		preserveAspectRatio="none"
		aria-hidden="true"
	>
		<rect width="1440" height="70" fill={bottomColor} />
		<path
			d="M0,0 L0,38
			   C40,42 70,46 120,44
			   C180,41 210,33 270,36
			   C320,38 355,47 410,48
			   C460,49 495,43 540,39
			   C590,34 620,31 680,34
			   C730,36 760,44 820,46
			   C870,47 910,42 960,38
			   C1010,34 1040,30 1090,33
			   C1130,35 1160,43 1210,45
			   C1260,47 1300,44 1340,40
			   C1370,37 1400,35 1440,36
			   L1440,0 Z"
			fill={topColor}
		/>
	</svg>
</div>

<style>
	.wavy-divider {
		width: 100%;
		line-height: 0;
		overflow: hidden;
	}

	svg {
		display: block;
		width: 100%;
		height: 70px;
	}
</style>
