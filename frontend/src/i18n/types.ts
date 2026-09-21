export type Locale = 'en' | 'zh-CN';

export interface Messages {
  sidebar: {
    brand: string;
    version: string;
    overview: string;
    usage: string;
    requests: string;
    cost: string;
    drilldown: string;
    keys: string;
    traffic: string;
    clientKeys: string;
    quotas: string;
    configConsole: string;
    navigation: string;
    openNavigation: string;
    closeNavigation: string;
    workspace: string;
    logout: string;
    sections: {
      main: string;
      config: string;
      monitor: string;
      admin: string;
      settings: string;
    };
    providers: string;
    keyPools: string;
    routing: string;
    modelCatalog: string;
    budgets: string;
    users: string;
    roles: string;
    auditLog: string;
    system: string;
  };

  overview: {
    title: string;
    subtitle: string;
    totalRequests: string;
    cacheHits: string;
    errors: string;
    activeConns: string;
    activeKeys: string;
    alerts24h: string;
    trafficTrend: string;
    keyHealth: string;
    noPools: string;
    requests: string;
  };
  cost: {
    title: string;
    subtitle: string;
    allTenants: string;
    requests24h: string;
    estCost: string;
    avgLatency: string;
    errorRate: string;
    totalErrors: string;
    promptTokens: string;
    completionTokens: string;
    cachedTokens: string;
    noData: string;
    thModel: string;
    thPool: string;
    thPrompt: string;
    thCompletion: string;
    thCacheHits: string;
    thRequests: string;
    thErrors: string;
    thCost: string;
    csv: string;
  };
  keys: {
    title: string;
    subtitle: string;
    healthy: string;
    healthyPct: string;
    healthyLabel: string;
    healthyBadge: string;
    excluded: string;
    weight: string;
    noPools: string;
  };
  traffic: {
    title: string;
    subtitle: string;
    allTenants: string;
    totalRequests: string;
    avgLatency: string;
    requestVolume: string;
    latencyTrend: string;
    requests: string;
    latency: string;
    csv: string;
    days: string;
  };
  drilldown: {
    back: string;
    unknownModel: string;
    subtitle: string;
    estCost: string;
    cacheHitRate: string;
    avgLatency: string;
    hourlyBreakdown: string;
    requests: string;
    promptTokens: string;
    completionTokens: string;
  };
  usage: {
    title: string;
    subtitle: string;
    allTenants: string;
    range24h: string;
    range7d: string;
    range30d: string;
    todayCost: string;
    todayRequests: string;
    todayErrors: string;
    todayAvgLatency: string;
    totalTokens: string;
    promptTokens: string;
    completionTokens: string;
    cachedTokens: string;
    costTrend: string;
    requestTrend: string;
    latencyTrend: string;
    tokenTrend: string;
    modelBreakdown: string;
    thModel: string;
    thRequests: string;
    thErrors: string;
    thTokens: string;
    thCacheHit: string;
    thAvgLatency: string;
    thCost: string;
    requestsLabel: string;
    costLabel: string;
    latencyLabel: string;
    promptLabel: string;
    completionLabel: string;
    cacheLabel: string;
  };
  requests: {
    title: string;
    subtitle: string;
    allTenants: string;
    allHistory: string;
    range24h: string;
    range7d: string;
    range30d: string;
    csv: string;
    noData: string;
    noDataHint: string;
    showing: string;
    thTime: string;
    thId: string;
    thModel: string;
    thPool: string;
    thStatus: string;
    thTokens: string;
    thLatency: string;
    thTtft: string;
    thRetry: string;
    thError: string;
    thCost: string;
    thFinish: string;
    thStream: string;
    detailTitle: string;
    streamYes: string;
    streamNo: string;
    fieldId: string;
    fieldTime: string;
    fieldModel: string;
    fieldPool: string;
    fieldTenant: string;
    fieldKeyHash: string;
    fieldStatus: string;
    fieldLatency: string;
    fieldTtft: string;
    fieldRetry: string;
    fieldStream: string;
    fieldFinish: string;
    fieldError: string;
    fieldErrorCode: string;
    fieldPrompt: string;
    fieldCompletion: string;
    fieldCached: string;
    fieldTotal: string;
    fieldCost: string;
    fieldUpstream: string;
    fieldUpstreamModel: string;
    fieldUserAgent: string;
    fieldClientIp: string;
    fieldFingerprint: string;
    fieldCacheSource: string;
    fieldReasoning: string;
    fieldAudio: string;
  };
  clientKeys: {
    title: string;
    subtitle: string;
    keysCount: string;
    addKey: string;
    newKey: string;
    rotateKey: string;
    create: string;
    cancel: string;
    rotate: string;
    newApiKey: string;
    apiKey: string;
    tenant: string;
    label: string;
    thHash: string;
    thTenant: string;
    thLabel: string;
    thStatus: string;
    thCreated: string;
    thActions: string;
    enabled: string;
    disabled: string;
    noKeys: string;
  };
  quotas: {
    title: string;
    subtitle: string;
    dailyTokens: string;
    monthlyRequests: string;
    critical: string;
    warning: string;
    noData: string;
    noDataHint: string;
  };
  adminUi: {
    add: string;
    create: string;
    edit: string;
    delete: string;
    cancel: string;
    save: string;
    retry: string;
    actions: string;
    confirmDelete: string;
    noProviders: string;
    noPools: string;
    noRouting: string;
    noModels: string;
    noUsers: string;
    unavailable: string;
    secretHint: string;
  };
  configAdmin: {
    configTitle: string;
    configSubtitle: string;
    exportConfig: string;
    validateImport: string;
    configHint: string;
    configPlaceholder: string;
    validate: string;
    dryRun: string;
    commitImport: string;
    refresh: string;
    confirmCommit: string;
    yamlRequired: string;
    notManaged: string;
    notManagedHint: string;
    version: string;
    providers: string;
    pools: string;
    routing: string;
    registry: string;
    providerTitle: string;
    providerSubtitle: string;
    providerEmptyHint: string;
    modelTitle: string;
    modelSubtitle: string;
    newModel: string;
    enabled: string;
    disabled: string;
    rolesTitle: string;
    rolesSubtitle: string;
    budgetTitle: string;
    budgetSubtitle: string;
    budgetHint: string;
    auditTitle: string;
    auditSubtitle: string;
    auditHint: string;
    addProvider: string;
    providerId: string;
    baseUrl: string;
    poolId: string;
    keyPoolsTitle: string;
    keyPoolsSubtitle: string;
    addPool: string;
    keyHash: string;
    weight: string;
    status: string;
    apiKey: string;
    addKey: string;
    routingTitle: string;
    routingSubtitle: string;
    context: string;
    output: string;
    vision: string;
    tools: string;
    jsonMode: string;
    yes: string;
    no: string;
    usersTitle: string;
    usersCount: string;
    addUser: string;
    createUser: string;
    name: string;
    email: string;
    password: string;
    roles: string;
    systemTitle: string;
    systemSubtitle: string;
    uptime: string;
    totalRequests: string;
    failedRequests: string;
    cacheHits: string;
    activeConnections: string;
    retries: string;
    keyDemotions: string;
    upstream5xx: string;
    upstream4xx: string;
    alertCount: string;
  };
  site: {
    nav: { product: string; models: string; documentation: string; status: string; signIn: string; openNavigation: string; closeNavigation: string; publicNavigation: string; publicMobileNavigation: string; accountNavigation: string; accountMobileNavigation: string; footerTagline: string; accountLabel: string },
    actions: { openWorkspace: string; unavailable: string; documentation: string },
    hero: { eyebrow: string; operationalTitle: string; operationalDescription: string; authorizationNote: string },
    public: { aboutBody: string; modelsBody: string; modelName: string; modelStatus: string; modelInput: string; modelOutput: string; modelCacheRead: string; modelsSubtitle: string; modelsNote: string; modelsNote2: string; statusBody: string; statusOperational: string; statusUnavailable: string; statusAllOk: string; statusAllOkEn: string; statusCoreServices: string; statusLast30d: string; contactBody: string; contactSupportBody: string; contactEyebrow: string; contactCardCta: string; docsBody: string; docsTopics: string[]; guideBody: string; unavailableBody: string; backHome: string },
    account: { groups: Record<'workspace' | 'developer' | 'commercial' | 'community' | 'activities' | 'operator', string>; overviewBody: string; unavailableBody: string; unavailableAction: string; tableField: string; tableState: string; emptyState: string; editAction: string; exportAction: string; familyTitle: string; familyBody: string; familyState: string; familyAction: string; operatorTitle: string; operatorBody: string; operatorSearch: string; operatorFilter: string; operatorCreate: string; operatorExport: string },
    routes: Record<string, { title: string; description: string; action?: string }>,
    fallback: { title: string; description: string },
    home: {
      nav: { signIn: string; signUp: string; console: string; admin: string; models: string; status: string; docs: string; about: string; contact: string },
      hero: { eyebrow: string; titleParts: { brand: string; mid: string; tail: string }; tagline: string; sub: string; activeOn: string; works: string[] },
      cta: { start: string; console: string; docs: string; viewPrice: string },
      channels: { tag: string; title: string; copyTitle: string; copyBody: string; items: { name: string; hint: string }[] },
      sections: { featuresTag: string; featuresTitle: string; imageTag: string; imageTitle: string; imageLede: string; pricingTag: string; codeTag: string; codeTitle: string; codeLede: string },
      onboard: { s1t: string; s1d: string; s2t: string; s2d: string; s3t: string; s3d: string; docLink: string; docLinkCta: string },
      image: { badge: string; model: string; desc: string; caps: string[]; docLink: string },
      features: { title: string; en: string; desc: string }[],
      pricing: { lineA: string; lineB: string; lineC: string; blurb: string; tags: string[] },
      footer: { tagline: string; docs: string; claudeCode: string; codex: string },
      closer: { title: string; sub: string },
      manifesto: { tag: string; title: string; integrity: string; body1: string; body2: string; pledges: { label: string; title: string; desc: string }[] },
      stats: { requests: string; uptime: string; latency: string },
    },
  };
  common: {
    csv: string;
    json: string;
    export: string;
    loading: string;
    placeholder_model: string;
    placeholder_pool: string;
    placeholder_finish: string;
    placeholder_error: string;
    placeholder_tenant: string;
    switchLang: string;
    uiClose: string;
    uiLoading: string;
    uiNoData: string;
    uiUnableToLoad: string;
    uiRetry: string;
    uiIncreasing: string;
    uiDecreasing: string;
    uiUnchanged: string;
    uiPagination: string;
    uiPrevious: string;
    uiNext: string;
  };
}
