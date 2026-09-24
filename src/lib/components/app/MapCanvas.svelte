<script lang="ts">
  /**
   * 百度 JS API GL 矢量底图，AutoBee 在上面叠加路线。
   * `BMapGL.Point` 是 (lng, lat)，与百度 Web 服务 API 的 `location=lat,lng` 相反；
   * 写反不报错，点会落到地球另一边，所以一律经 `pt()` 构造。
   * 底图是 BD-09：`plan.crs` 不是 `BD09` 时路线整体偏移几百米且不报错，
   * 所以坐标系不匹配时显式提示，不静默画。
   */
  import { planner } from "$lib/state/planner.svelte";
  import type { Crs, LatLng, RoutePlan } from "$lib/ipc/types";

  type Point = { lng: number; lat: number };
  type Overlay = object;
  type BaiduMap = {
    centerAndZoom(center: Point, zoom: number): void;
    setCenter(center: Point): void;
    setZoom(zoom: number): void;
    getZoom(): number;
    setViewport(points: Point[], options?: { margins?: number[] }): void;
    addOverlay(overlay: Overlay): void;
    removeOverlay(overlay: Overlay): void;
    enableScrollWheelZoom(enable?: boolean): void;
    destroy(): void;
  };
  type BaiduApi = {
    Point: new (lng: number, lat: number) => Point;
    Map: new (element: HTMLElement, options?: Record<string, unknown>) => BaiduMap;
    Polyline: new (points: Point[], options: Record<string, unknown>) => Overlay;
    Marker: new (point: Point, options?: Record<string, unknown>) => Overlay;
    Icon: new (url: string, size: Point | object, options?: Record<string, unknown>) => object;
    Size: new (width: number, height: number) => object;
  };

  let { class: className = "" }: { class?: string } = $props();
  let container = $state<HTMLDivElement | null>(null);
  let map: BaiduMap | null = null;
  let routeOverlays: Overlay[] = [];
  let markerOverlays: Overlay[] = [];
  let loadError = $state("");
  let mapReady = $state(false);
  let crsWarning = $state("");

  function crsMismatch(crs: Crs): string {
    return `路线坐标系是 ${crs},百度底图是 BD09:线会整体偏移。到设置里把输出坐标系改成 BD09。`;
  }

  function sdk(): BaiduApi | undefined {
    return (window as unknown as { BMapGL?: BaiduApi }).BMapGL;
  }

  function pt(location: LatLng): Point {
    const BMapGL = sdk();
    if (!BMapGL) throw new Error("BMapGL 未加载");
    return new BMapGL.Point(location.lng, location.lat);
  }

  $effect(() => {
    if (container) void initMap(container);
    // destroy() 是硬要求：组件在路由切换时反复挂载，不调则 WebGL 上下文、监听与瓦片缓存留在内存里。
    return () => {
      routeOverlays.length = 0;
      markerOverlays.length = 0;
      map?.destroy();
      map = null;
      mapReady = false;
    };
  });

  $effect(() => {
    void planner.selectedRouteId;
    void planner.circleResults;
    if (map && mapReady) {
      renderPlan(planner.plan);
      renderMarkers(planner.plan);
    }
  });

  async function initMap(element: HTMLElement): Promise<void> {
    try {
      const BMapGL = await loadSdk();
      // GL 版只认 enableIconClick，写 enableMapClick 会被静默忽略。
      map = new BMapGL.Map(element, { enableIconClick: false });
      // centerAndZoom 必须先调：没调过的 Map 是未初始化状态，后续 addOverlay 全部无效。
      map.centerAndZoom(new BMapGL.Point(116.404, 39.915), 12);
      map.enableScrollWheelZoom(true);
      mapReady = true;
      renderPlan(planner.plan);
      renderMarkers(planner.plan);
    } catch (error) {
      loadError = error instanceof Error ? error.message : "百度地图加载失败";
    }
  }

  // 不用官方 `api?v=3.0&type=webgl` 入口：它内部是 `document.write`，页面加载完后执行会冲掉整个文档。
  // 改为注入其指向的 `getscript` 与 `bmap.css`（缺 CSS 的表现是控件与文字标注错位，不是白屏）。
  async function loadSdk(): Promise<BaiduApi> {
    const existing = sdk();
    if (existing) return existing;
    const ak = await planner.mapSdkKey();
    if (!ak) throw new Error("还没填百度 AK：打开设置，把百度地图的 AK 填进去再回来。");

    if (!document.querySelector('link[data-bmapgl-ui="gl"]')) {
      const css = document.createElement("link");
      css.rel = "stylesheet";
      css.type = "text/css";
      css.dataset.bmapglUi = "gl";
      css.href = "https://api.map.baidu.com/res/webgl/10/bmap.css";
      document.head.append(css);
    }
    await new Promise<void>((resolve, reject) => {
      const script = document.createElement("script");
      script.src = `https://api.map.baidu.com/getscript?type=webgl&v=1.0&ak=${encodeURIComponent(ak)}&services=&t=`;
      script.charset = "utf-8";
      script.onload = () => resolve();
      script.onerror = () => reject(new Error("百度地图 SDK 脚本加载失败：检查网络，或 AK 是否配了 Referer 白名单。"));
      document.head.append(script);
    });
    // 脚本 onload 早于 BMapGL 挂上 window 一个微任务，轮询几轮而不是直接读。
    for (let waited = 0; waited < 15_000; waited += 50) {
      const current = sdk();
      if (current) return current;
      await new Promise((resolve) => window.setTimeout(resolve, 50));
    }
    throw new Error("百度地图 SDK 加载超时");
  }

  /** BMapGL 不会替你回收叠加物，漏 removeOverlay 就是越画越花。 */
  function clearOverlays(overlays: Overlay[]): void {
    if (!map) return;
    for (const overlay of overlays) map.removeOverlay(overlay);
    overlays.length = 0;
  }

  function renderPlan(plan: RoutePlan | null): void {
    const BMapGL = sdk();
    if (!map || !BMapGL) return;
    clearOverlays(routeOverlays);
    if (!plan || plan.routes.length === 0) return;
    crsWarning = plan.crs === "BD09" ? "" : crsMismatch(plan.crs);
    const route = plan.routes.find((item) => item.id === planner.selectedRouteId) ?? plan.routes[0];
    for (const segment of route.segments) {
      if (segment.points.length < 2) continue;
      const isWalk = segment.kind === "walk";
      const line = new BMapGL.Polyline(segment.points.map(pt), {
        strokeColor: segment.line?.color ?? "#9aa0a6",
        strokeWeight: isWalk ? 4 : 7,
        strokeOpacity: 0.95,
        strokeStyle: isWalk ? "dashed" : "solid",
      });
      map.addOverlay(line);
      routeOverlays.push(line);
    }
    const routeBounds = route.bounds;
    const boundsAreValid = routeBounds.every((point) =>
      Number.isFinite(point.lat) && Number.isFinite(point.lng)
      && Math.abs(point.lat) <= 90 && Math.abs(point.lng) <= 180,
    );
    // setViewport 按点集自算中心与级别，不必再 setCenter；margins 是上右下左留白像素。
    const viewport = boundsAreValid
      ? routeBounds
      : [plan.origin.location, plan.destination.location];
    map.setViewport(viewport.map(pt), { margins: [72, 72, 72, 72] });
  }

  /** 跟 `renderPlan` 分开：生活圈有自己的地点输入，写在 renderPlan 里会被它的 `!plan` 提前返回吞掉。 */
  function renderMarkers(plan: RoutePlan | null): void {
    const BMapGL = sdk();
    if (!map || !BMapGL) return;
    clearOverlays(markerOverlays);

    /** anchor 让针尖而不是图片中心落在坐标上。 */
    const place = (location: LatLng, src: string, w: number, h: number, anchorX: number, anchorY: number): void => {
      const icon = new BMapGL.Icon(src, new BMapGL.Size(w, h), {
        anchor: new BMapGL.Size(anchorX, anchorY),
        imageSize: new BMapGL.Size(w, h),
      });
      const marker = new BMapGL.Marker(pt(location), { icon });
      map!.addOverlay(marker);
      markerOverlays.push(marker);
    };

    if (plan) {
      place(plan.origin.location, pin("#16a34a"), 28, 36, 14, 36);
      place(plan.destination.location, pin("#dc2626"), 28, 36, 14, 36);
    }
    const pois = planner.reachablePois;
    const center = planner.circleOrigin;
    if (center && pois.length > 0 && !plan) {
      place(center.location, pin("#7c3aed"), 28, 36, 14, 36);
    }
    for (const poi of pois) {
      place(poi.location, dot("#2563eb"), 14, 14, 7, 7);
    }
    if (!plan && center && pois.length > 0) {
      map.setCenter(pt(center.location));
      if (map.getZoom() < 14) map.setZoom(14);
    }
  }


  function pin(color: string): string {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="28" height="36"><path d="M14 0C6.3 0 0 6.3 0 14c0 10 14 22 14 22s14-12 14-22C28 6.3 21.7 0 14 0z" fill="${color}"/><circle cx="14" cy="13.5" r="5.5" fill="#fff"/></svg>`;
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  }

  function dot(color: string): string {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14"><circle cx="7" cy="7" r="5" fill="#fff" stroke="${color}" stroke-width="3"/></svg>`;
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  }

  function zoomBy(delta: number): void {
    if (map) map.setZoom(map.getZoom() + delta);
  }
</script>

<div bind:this={container} class={`relative overflow-hidden rounded-lg border ${className}`}>
  {#if loadError}<div class="absolute inset-0 z-10 grid place-items-center bg-muted p-6 text-center text-sm text-muted-foreground">{loadError}</div>{/if}
  {#if !loadError && crsWarning}
    <div role="alert" class="absolute inset-x-3 top-3 z-10 rounded-md border border-warning bg-card px-3 py-2 text-sm text-warning shadow-sm">{crsWarning}</div>
  {/if}
  <!-- 比赛要求底图来源可见。 -->
  <div class="absolute bottom-1 left-2 z-10 text-[11px] text-muted-foreground">底图 © 百度地图</div>
  <div class="absolute right-3 bottom-3 z-10 flex flex-col overflow-hidden rounded-md border bg-card shadow-sm">
    <button type="button" aria-label="放大" class="size-8 text-lg hover:bg-accent" onclick={() => zoomBy(1)}>+</button>
    <button type="button" aria-label="缩小" class="size-8 border-t text-lg hover:bg-accent" onclick={() => zoomBy(-1)}>−</button>
  </div>
</div>
