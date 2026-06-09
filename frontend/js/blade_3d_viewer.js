class Blade3DViewer {
    constructor(containerId, apiClient) {
        this.containerId = containerId;
        this.api = apiClient;
        this.container = null;
        this.scene = null;
        this.camera = null;
        this.renderer = null;
        this.bladeMesh = null;
        this.aePoints = [];
        this.sectionMarkers = [];
        this.raycaster = new THREE.Raycaster();
        this.mouse = new THREE.Vector2();
        this.displayMode = 'heatmap';
        this.currentSection = 'mid';
        this.heatmapRange = 2000;
        this.strainData = [];
        this.aeEventData = [];
        this.onSectionClick = null;
        this.animationId = null;
        this.clock = new THREE.Clock();
        this.isInitialized = false;
        this.currentTurbine = 'WT001';
        this.currentBlade = 'A';

        this.heatmapColors = [
            { r: 0.13, g: 0.34, b: 0.48 },
            { r: 0.22, g: 0.64, b: 0.65 },
            { r: 0.34, g: 0.80, b: 0.60 },
            { r: 0.50, g: 0.93, b: 0.60 },
            { r: 0.78, g: 0.98, b: 0.80 },
            { r: 1.00, g: 1.00, b: 0.00 },
            { r: 1.00, g: 0.82, b: 0.00 },
            { r: 1.00, g: 0.58, b: 0.00 },
            { r: 1.00, g: 0.42, b: 0.00 },
            { r: 1.00, g: 0.00, b: 0.00 }
        ];

        this.vertexShader = `
            varying vec3 vNormal;
            varying vec3 vPosition;
            varying vec3 vStrainColor;
            attribute vec3 strainColor;
            uniform float uTime;
            
            void main() {
                vNormal = normalize(normalMatrix * normal);
                vPosition = position;
                vStrainColor = strainColor;
                
                vec3 pos = position;
                float wave = sin(pos.z * 0.5 + uTime * 0.5) * 0.02;
                pos.y += wave;
                
                gl_Position = projectionMatrix * modelViewMatrix * vec4(pos, 1.0);
            }
        `;

        this.fragmentShader = `
            varying vec3 vNormal;
            varying vec3 vPosition;
            varying vec3 vStrainColor;
            uniform float uShowHeatmap;
            uniform float uShowAE;
            
            void main() {
                vec3 lightDir = normalize(vec3(1.0, 1.0, 1.0));
                float diff = max(dot(vNormal, lightDir), 0.0);
                vec3 ambient = vec3(0.15, 0.18, 0.25);
                vec3 baseColor = mix(vec3(0.3, 0.35, 0.4), vStrainColor, uShowHeatmap);
                vec3 finalColor = ambient + baseColor * diff * 0.8;
                
                float rim = 1.0 - max(dot(vNormal, vec3(0.0, 0.0, 1.0)), 0.0);
                finalColor += rim * vec3(0.0, 0.8, 1.0) * 0.3;
                
                gl_FragColor = vec4(finalColor, 0.95);
            }
        `;
    }

    init() {
        this.container = document.getElementById(this.containerId);
        if (!this.container || this.isInitialized) return;

        const rect = this.container.getBoundingClientRect();
        const width = rect.width || 800;
        const height = rect.height || 600;

        this.scene = new THREE.Scene();
        this.scene.background = new THREE.Color(0x0a0e1a);
        this.scene.fog = new THREE.Fog(0x0a0e1a, 10, 50);

        this.camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 1000);
        this.camera.position.set(0, 5, 25);
        this.camera.lookAt(0, 0, 0);

        this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
        this.renderer.setSize(width, height);
        this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        this.renderer.shadowMap.enabled = true;
        this.renderer.shadowMap.type = THREE.PCFSoftShadowMap;
        this.container.appendChild(this.renderer.domElement);

        this.setupLights();
        this.createBlade();
        this.createSectionMarkers();
        this.setupControls();
        this.setupEventListeners();

        this.isInitialized = true;
        this.animate();

        console.log(`[Blade3DViewer] 初始化完成: ${this.containerId}`);
    }

    setupLights() {
        const ambientLight = new THREE.AmbientLight(0x404050, 0.6);
        this.scene.add(ambientLight);

        const mainLight = new THREE.DirectionalLight(0xffffff, 1.2);
        mainLight.position.set(5, 10, 7);
        mainLight.castShadow = true;
        mainLight.shadow.mapSize.width = 2048;
        mainLight.shadow.mapSize.height = 2048;
        this.scene.add(mainLight);

        const fillLight = new THREE.DirectionalLight(0x00d4ff, 0.4);
        fillLight.position.set(-5, 3, -5);
        this.scene.add(fillLight);

        const rimLight = new THREE.DirectionalLight(0x7c3aed, 0.3);
        rimLight.position.set(0, -3, -10);
        this.scene.add(rimLight);

        const pointLight1 = new THREE.PointLight(0x00d4ff, 0.5, 30);
        pointLight1.position.set(3, 2, 3);
        this.scene.add(pointLight1);

        const pointLight2 = new THREE.PointLight(0x7c3aed, 0.3, 20);
        pointLight2.position.set(-3, 1, 2);
        this.scene.add(pointLight2);
    }

    createBlade() {
        const bladeLength = 20;
        const rootWidth = 2.5;
        const tipWidth = 0.3;
        const segments = 50;

        const shape = new THREE.Shape();
        shape.moveTo(0, -rootWidth / 2);
        shape.quadraticCurveTo(rootWidth / 4, -rootWidth / 2, rootWidth / 2, 0);
        shape.quadraticCurveTo(rootWidth / 4, rootWidth / 2, 0, rootWidth / 2);
        shape.lineTo(0, -rootWidth / 2);

        const extrudeSettings = {
            steps: segments,
            depth: bladeLength,
            bevelEnabled: false,
            extrudePath: new THREE.CatmullRomCurve3([
                new THREE.Vector3(0, 0, 0),
                new THREE.Vector3(0.5, 0.5, bladeLength * 0.3),
                new THREE.Vector3(0.8, 1.0, bladeLength * 0.6),
                new THREE.Vector3(0.5, 0.5, bladeLength * 0.85),
                new THREE.Vector3(0, 0, bladeLength)
            ])
        };

        const geometry = new THREE.ExtrudeGeometry(shape, extrudeSettings);
        geometry.center();
        geometry.rotateX(-Math.PI / 2);
        geometry.rotateZ(-Math.PI / 6);

        const count = geometry.attributes.position.count;
        const strainColors = new Float32Array(count * 3);
        geometry.setAttribute('strainColor', new THREE.BufferAttribute(strainColors, 3));

        const material = new THREE.ShaderMaterial({
            uniforms: {
                uTime: { value: 0 },
                uHeatmapRange: { value: this.heatmapRange },
                uShowHeatmap: { value: 1.0 },
                uShowAE: { value: 0.0 }
            },
            vertexShader: this.vertexShader,
            fragmentShader: this.fragmentShader,
            transparent: true,
            side: THREE.DoubleSide
        });

        this.bladeMesh = new THREE.Mesh(geometry, material);
        this.bladeMesh.castShadow = true;
        this.bladeMesh.receiveShadow = true;
        this.scene.add(this.bladeMesh);

        const wireframeGeometry = new THREE.WireframeGeometry(geometry);
        const wireframeMaterial = new THREE.LineBasicMaterial({
            color: 0x00d4ff,
            transparent: true,
            opacity: 0.1
        });
        const wireframe = new THREE.LineSegments(wireframeGeometry, wireframeMaterial);
        this.bladeMesh.add(wireframe);

        this.generateStrainData();
        this.updateStrainColors();
    }

    createSectionMarkers() {
        const sections = [
            { name: 'root', z: -7, color: 0x10b981 },
            { name: 'mid', z: 0, color: 0x00d4ff },
            { name: 'tip', z: 7, color: 0xef4444 }
        ];

        sections.forEach(section => {
            const ringGeometry = new THREE.TorusGeometry(1.5, 0.05, 16, 32);
            const ringMaterial = new THREE.MeshBasicMaterial({
                color: section.color,
                transparent: true,
                opacity: 0.6
            });
            const ring = new THREE.Mesh(ringGeometry, ringMaterial);
            ring.position.set(0, 0, section.z);
            ring.rotation.x = Math.PI / 2;
            ring.userData = { type: 'section', name: section.name };

            this.bladeMesh.add(ring);
            this.sectionMarkers.push({ mesh: ring, ...section });

            const glowGeometry = new THREE.RingGeometry(1.4, 1.6, 32);
            const glowMaterial = new THREE.MeshBasicMaterial({
                color: section.color,
                transparent: true,
                opacity: 0.2,
                side: THREE.DoubleSide
            });
            const glow = new THREE.Mesh(glowGeometry, glowMaterial);
            glow.position.set(0, 0, section.z);
            glow.rotation.x = Math.PI / 2;
            this.bladeMesh.add(glow);
        });
    }

    setupControls() {
        let isDragging = false;
        let previousMousePosition = { x: 0, y: 0 };
        let spherical = { theta: Math.PI / 4, phi: Math.PI / 3, radius: 25 };
        const target = new THREE.Vector3(0, 0, 0);

        const updateCamera = () => {
            this.camera.position.x = target.x + spherical.radius * Math.sin(spherical.phi) * Math.cos(spherical.theta);
            this.camera.position.y = target.y + spherical.radius * Math.cos(spherical.phi);
            this.camera.position.z = target.z + spherical.radius * Math.sin(spherical.phi) * Math.sin(spherical.theta);
            this.camera.lookAt(target);
        };

        this.container.addEventListener('mousedown', (e) => {
            isDragging = true;
            previousMousePosition = { x: e.clientX, y: e.clientY };
        });

        this.container.addEventListener('mousemove', (e) => {
            const deltaMove = {
                x: e.clientX - previousMousePosition.x,
                y: e.clientY - previousMousePosition.y
            };

            if (isDragging && e.buttons === 1) {
                spherical.theta -= deltaMove.x * 0.005;
                spherical.phi = Math.max(0.1, Math.min(Math.PI - 0.1, spherical.phi + deltaMove.y * 0.005));
                updateCamera();
            } else if (isDragging && e.buttons === 2) {
                const right = new THREE.Vector3();
                const up = new THREE.Vector3(0, 1, 0);
                this.camera.getWorldDirection(right);
                right.cross(up).normalize();
                target.addScaledVector(right, -deltaMove.x * 0.01);
                target.y += deltaMove.y * 0.01;
                updateCamera();
            }

            previousMousePosition = { x: e.clientX, y: e.clientY };

            const rect = this.container.getBoundingClientRect();
            this.mouse.x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
            this.mouse.y = -((e.clientY - rect.top) / rect.height) * 2 + 1;
            this.checkSectionHover();
        });

        this.container.addEventListener('mouseup', () => {
            isDragging = false;
        });

        this.container.addEventListener('mouseleave', () => {
            isDragging = false;
        });

        this.container.addEventListener('wheel', (e) => {
            e.preventDefault();
            spherical.radius = Math.max(10, Math.min(50, spherical.radius + e.deltaY * 0.01));
            updateCamera();
        }, { passive: false });

        this.container.addEventListener('click', (e) => {
            if (Math.abs(e.movementX) < 5 && Math.abs(e.movementY) < 5) {
                this.checkSectionClick();
            }
        });

        this.container.addEventListener('contextmenu', (e) => {
            e.preventDefault();
        });

        updateCamera();
    }

    setupEventListeners() {
        window.addEventListener('resize', () => {
            if (!this.container || !this.camera || !this.renderer) return;
            const rect = this.container.getBoundingClientRect();
            const width = rect.width || 800;
            const height = rect.height || 600;
            this.camera.aspect = width / height;
            this.camera.updateProjectionMatrix();
            this.renderer.setSize(width, height);
        });
    }

    generateStrainData() {
        this.strainData = [];
        const positions = this.bladeMesh.geometry.attributes.position;
        const count = positions.count;

        for (let i = 0; i < count; i++) {
            const z = positions.getZ(i);
            const zNormalized = (z + 10) / 20;
            const baseStrain = 500 + zNormalized * 1000;
            const noise = Math.random() * 200;
            this.strainData.push(baseStrain + noise);
        }
    }

    getHeatmapColor(strain, z) {
        const normalized = Math.min(1, Math.max(0, strain / this.heatmapRange));
        const index = Math.min(this.heatmapColors.length - 2, Math.floor(normalized * (this.heatmapColors.length - 1)));
        const t = (normalized * (this.heatmapColors.length - 1)) % 1;

        const c1 = this.heatmapColors[index];
        const c2 = this.heatmapColors[index + 1];

        return {
            r: c1.r + (c2.r - c1.r) * t,
            g: c1.g + (c2.g - c1.g) * t,
            b: c1.b + (c2.b - c1.b) * t
        };
    }

    updateStrainColors() {
        if (!this.bladeMesh) return;

        const colors = this.bladeMesh.geometry.attributes.strainColor;
        const positions = this.bladeMesh.geometry.attributes.position;

        for (let i = 0; i < colors.count; i++) {
            const strain = this.strainData[i] || 0;
            const z = positions.getZ(i);
            const color = this.getHeatmapColor(strain, z);
            colors.setXYZ(i, color.r, color.g, color.b);
        }

        colors.needsUpdate = true;
    }

    addAEEvent(x, y, z, amplitude) {
        const geometry = new THREE.SphereGeometry(0.1 + amplitude / 200, 16, 16);
        const material = new THREE.MeshBasicMaterial({
            color: new THREE.Color().setHSL(0.15 - amplitude / 400, 1, 0.6),
            transparent: true,
            opacity: 0.9
        });

        const sphere = new THREE.Mesh(geometry, material);
        sphere.position.set(x, y, z);
        sphere.userData = {
            type: 'aeEvent',
            amplitude,
            createdAt: Date.now(),
            lifetime: 3000 + Math.random() * 2000
        };

        const glowGeometry = new THREE.SphereGeometry(0.2 + amplitude / 100, 16, 16);
        const glowMaterial = new THREE.MeshBasicMaterial({
            color: 0xffff00,
            transparent: true,
            opacity: 0.3
        });
        const glow = new THREE.Mesh(glowGeometry, glowMaterial);
        sphere.add(glow);

        this.bladeMesh.add(sphere);
        this.aePoints.push(sphere);
    }

    generateRandomAEEvents() {
        if (this.displayMode === 'heatmap' || Math.random() > 0.1) return;

        const z = (Math.random() - 0.5) * 18;
        const angle = Math.random() * Math.PI * 2;
        const radius = 0.5 + Math.random() * 1.0;
        const x = Math.cos(angle) * radius;
        const y = Math.sin(angle) * radius;
        const amplitude = 70 + Math.random() * 40;

        this.addAEEvent(x, y, z, amplitude);
    }

    updateAEEvents() {
        const now = Date.now();
        this.aePoints = this.aePoints.filter(point => {
            const age = now - point.userData.createdAt;
            const lifetime = point.userData.lifetime;

            if (age > lifetime) {
                this.bladeMesh.remove(point);
                return false;
            }

            const progress = age / lifetime;
            point.material.opacity = 0.9 * (1 - progress);
            point.scale.setScalar(1 + progress * 0.5);

            if (point.children[0]) {
                point.children[0].material.opacity = 0.3 * (1 - progress);
            }

            return true;
        });
    }

    checkSectionHover() {
        if (!this.bladeMesh) return;

        this.raycaster.setFromCamera(this.mouse, this.camera);
        const intersects = this.raycaster.intersectObjects(this.sectionMarkers.map(s => s.mesh));

        this.sectionMarkers.forEach(section => {
            section.mesh.material.opacity = 0.6;
            section.mesh.scale.setScalar(1);
        });

        if (intersects.length > 0) {
            const section = this.sectionMarkers.find(s => s.mesh === intersects[0].object);
            if (section) {
                section.mesh.material.opacity = 1;
                section.mesh.scale.setScalar(1.2);
                this.container.style.cursor = 'pointer';
            }
        } else {
            this.container.style.cursor = 'grab';
        }
    }

    checkSectionClick() {
        if (!this.bladeMesh) return;

        this.raycaster.setFromCamera(this.mouse, this.camera);
        const intersects = this.raycaster.intersectObjects(this.sectionMarkers.map(s => s.mesh));

        if (intersects.length > 0) {
            const section = this.sectionMarkers.find(s => s.mesh === intersects[0].object);
            if (section && this.onSectionClick) {
                this.onSectionClick(section.name, this.currentTurbine, this.currentBlade);
            }
        }
    }

    setDisplayMode(mode) {
        this.displayMode = mode;
        if (this.bladeMesh) {
            if (mode === 'heatmap') {
                this.bladeMesh.material.uniforms.uShowHeatmap.value = 1.0;
                this.bladeMesh.material.uniforms.uShowAE.value = 0.0;
            } else if (mode === 'ae') {
                this.bladeMesh.material.uniforms.uShowHeatmap.value = 0.0;
                this.bladeMesh.material.uniforms.uShowAE.value = 1.0;
            } else {
                this.bladeMesh.material.uniforms.uShowHeatmap.value = 1.0;
                this.bladeMesh.material.uniforms.uShowAE.value = 1.0;
            }
        }
    }

    setCurrentSection(section) {
        this.currentSection = section;

        this.sectionMarkers.forEach(s => {
            if (s.name === section) {
                s.mesh.material.opacity = 1;
                s.mesh.scale.setScalar(1.3);
            } else {
                s.mesh.material.opacity = 0.4;
                s.mesh.scale.setScalar(0.8);
            }
        });
    }

    setHeatmapRange(range) {
        this.heatmapRange = range;
        if (this.bladeMesh) {
            this.bladeMesh.material.uniforms.uHeatmapRange.value = range;
        }
        this.updateStrainColors();
    }

    setCurrentBlade(turbineId, bladeId) {
        this.currentTurbine = turbineId;
        this.currentBlade = bladeId;
        this.loadBladeData();
    }

    async loadBladeData() {
        console.log(`[Blade3DViewer] 加载数据: ${this.currentTurbine}-${this.currentBlade}`);

        try {
            const health = await this.api.getBladeHealth(this.currentTurbine, this.currentBlade);
            if (health && health.success && health.data) {
                this.generateStrainData();
                this.updateStrainColors();
            }
        } catch (e) {
            console.warn('[Blade3DViewer] 加载数据失败，使用模拟数据', e);
        }
    }

    updateStrainData(newData) {
        this.strainData = newData;
        this.updateStrainColors();
    }

    animate() {
        this.animationId = requestAnimationFrame(() => this.animate());

        const delta = this.clock.getDelta();
        const elapsed = this.clock.getElapsedTime();

        if (this.bladeMesh && this.bladeMesh.material.uniforms) {
            this.bladeMesh.material.uniforms.uTime.value = elapsed;
        }

        this.generateRandomAEEvents();
        this.updateAEEvents();

        if (this.renderer && this.scene && this.camera) {
            this.renderer.render(this.scene, this.camera);
        }
    }

    destroy() {
        if (this.animationId) {
            cancelAnimationFrame(this.animationId);
        }
        if (this.renderer) {
            this.renderer.dispose();
            if (this.container && this.renderer.domElement.parentNode === this.container) {
                this.container.removeChild(this.renderer.domElement);
            }
        }
        this.isInitialized = false;
        console.log(`[Blade3DViewer] 已销毁: ${this.containerId}`);
    }
}

if (typeof window !== 'undefined') {
    window.Blade3DViewer = Blade3DViewer;
}
