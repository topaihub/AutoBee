<script lang="ts">
  /**
   * 百度 JS API GL 矢量底图：底图自带道路、地铁线路与站点标注；AutoBee 叠加当前路线。
   *
   * # 坐标顺序的坑
   *
   * `BMapGL.Point` 是 **(lng, lat)**，经度在前 —— 与百度 Web 服务 API 的
   * `location=lat,lng` 相反(同一家两套接口两个顺序)。写反了不报错，
   * 点直接落到地球另一边。本文件一律经由 `pt()` 构造，不散写。
   *
   * # 坐标系
   *
   * BMapGL 底图是 BD-09。后端返回的 `plan.crs` 必须是 `BD09`，否则整条路线
   * 相对底图偏移几百米且不报错 —— 所以坐标系不匹配时显式提示，不静默画。
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
  /** 叠加物要自己记着：BMapGL 只有 add/removeOverlay，没有腾讯那种可整体 setGeometries 的图层。 */
  let routeOverlays: Overlay[] = [];
  let markerOverlays: Overlay[] = [];
  let loadError = $state("");
  let mapReady = $state(false);
  /**
   * 坐标系不匹配的提示。BMapGL 底图是 BD-09,后端按设置页的 `out_crs` 转换 --
   * 两者不一致时路线相对底图偏移几百米,而**不会报任何错**。静默画出来
   * 比不画更坏:用户会照着错的线去坐车。
   */
  let crsWarning = $state("");

  function crsMismatch(crs: Crs): string {
    return `路线坐标系是 ${crs},百度底图是 BD09:线会整体偏移。到设置里把输出坐标系改成 BD09。`;
  }

  function sdk(): BaiduApi | undefined {
    return (window as unknown as { BMapGL?: BaiduApi }).BMapGL;
  }

  /** 唯一的 `Point` 构造入口。经度在前 —— 见文件头注释。 */
  function pt(location: LatLng): Point {
    const BMapGL = sdk();
    if (!BMapGL) throw new Error("BMapGL 未加载");
    return new BMapGL.Point(location.lng, location.lat);
  }

  $effect(() => {
    if (container) void initMap(container);
    // destroy() 是官方明写的硬要求：不调的话 WebGL 上下文、事件监听与瓦片
    // 缓存都留在内存里。组件在路由切换时会反复挂载,漏一次积一份。
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
      // 键名必须是 enableIconClick，不能写 enableMapClick:SDK 的
      // compatMapOptions 把后者翻成前者的分支条件是 `if(!apiVersionIsGL())`,
      // GL 版压根不进那个分支 -- 写 enableMapClick 会被静默忽略,底图 POI
      // 仍然可点。这一条官方文档的选项表里也只列 enableIconClick。
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

  /**
   * 取 AK 并注入 SDK。
   *
   * 不用官方的 `api?v=3.0&type=webgl` 入口:那个 loader 内部是 `document.write`,
   * 页面加载完之后再执行会把整个文档冲掉。直接注入它内部指向的 `getscript`
   * 与配套 `bmap.css`(缺 CSS 的表现是控件与文字标注错位,不是白屏)。
   */
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

  /** 清掉一批叠加物并把数组清空。BMapGL 不会替你回收，漏了就是越画越花。 */
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
        // 步行段画虚线：与地铁/公交段并排时一眼能分出来，不靠记颜色。
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
    // setViewport 替代腾讯的 fitBounds：它按传入点集自己算中心与级别，
    // 所以不必再 setCenter + 夹 zoom。margins 是上右下左的留白像素。
    const viewport = boundsAreValid
      ? routeBounds
      : [plan.origin.location, plan.destination.location];
    map.setViewport(viewport.map(pt), { margins: [72, 72, 72, 72] });
  }

  /**
   * 端点与生活圈服务点的打点。跟 `renderPlan` 分开是必须的:生活圈现在有
   * 自己的地点输入,没规划过路线时也要能在图上看到服务点 -- 写在 renderPlan
   * 里会被它的 `!plan` 提前返回吞掉。
   */
  function renderMarkers(plan: RoutePlan | null): void {
    const BMapGL = sdk();
    if (!map || !BMapGL) return;
    clearOverlays(markerOverlays);

    /** 一个 SVG data URL 一个 Icon。anchor 让针尖而不是图片中心落在坐标上。 */
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
    // 没有路线时视野不会被 renderPlan 摆好,自己对到生活圈中心上。
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
  <!-- 数据来源标注:比赛要求可见地标出底图来源,也让用户知道这张图是谁给的。 -->
  <div class="absolute bottom-1 left-2 z-10 text-[11px] text-muted-foreground">底图 © 百度地图</div>
  <div class="absolute right-3 bottom-3 z-10 flex flex-col overflow-hidden rounded-md border bg-card shadow-sm">
    <button type="button" aria-label="放大" class="size-8 text-lg hover:bg-accent" onclick={() => zoomBy(1)}>+</button>
    <button type="button" aria-label="缩小" class="size-8 border-t text-lg hover:bg-accent" onclick={() => zoomBy(-1)}>−</button>
  </div>
</div>
