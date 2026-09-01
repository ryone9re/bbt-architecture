# BBT Architecture

Business / Boundary / Technology Architecture

BBT Architectureは、この文書で定義するアーキテクチャ指針である。

アプリケーションのコードをBusiness、Boundary、Technologyという3つの関心に分ける。Boundaryは方向によってEntry PointとIntegrationに分ける。実行可能なアプリケーションは、これらの外側にあるAppsで組み立てる。

この構成が目指すのは、業務の意味、外部との接続、具体的な技術を、それぞれ別の理由で理解し、変更できる状態である。

```text
Businessは業務を担う。
Technologyは技術を担う。
Boundaryは両者を接続する。
Appsはそれらを組み立てて実行する。
```

```text
Business does business.
Technology does technology.
Boundaries translate between them.
Apps assemble and run them.
```

## 1. 全体構造

再利用可能なコードは、次の4領域に分ける。

```text
src/
├── business/
├── entrypoint/
├── integration/
└── infrastructure/
```

- `business` は、業務操作、業務ルール、業務が外部へ求める能力を表す
- `entrypoint` は、外部からの入力をBusinessの操作へ接続する
- `integration` は、Businessが求める能力を具体的な技術で実現する
- `infrastructure` は、具体的な技術と、その技術上の表現を提供する

実行可能なコードは `apps` に置く。

```text
project/
├── apps/
│   ├── server/
│   ├── cli/
│   ├── worker/
│   └── migrate/
└── src/
    ├── business/
    ├── entrypoint/
    ├── integration/
    └── infrastructure/
```

実行時の流れは次のようになる。

```text
External World
    ↓
Infrastructure Runtime
HTTP Server / CLI Runtime / Message Consumer
    ↓
Entry Point
    ↓
Business Operation
    ↓
Integration
    ↓
Infrastructure Technology
Database / Cache / Storage / External API
    ↓
External World
```

コード上の依存方向は次のとおりである。

```text
apps ─────────────→ entrypoint
  ├───────────────→ business
  ├───────────────→ integration
  └───────────────→ infrastructure

entrypoint ───────→ business
     └────────────→ infrastructure

integration ──────→ business
     └────────────→ infrastructure

business ─────────→ Business内のコードと一般的なライブラリ
infrastructure ───→ Infrastructure内のコードとTechnologyライブラリ
```

`apps` は依存グラフの根になる。依存は `apps` から `src` へ向かう。

BusinessはEntry Point、Integration、Infrastructureの存在を知らない。

InfrastructureはBusiness、Entry Point、Integrationの型やモジュールを知らない。

Entry PointとIntegrationはBoundaryであるため、BusinessとTechnologyの両方を知る。

## 2. ディレクトリの分類軸

トップレベルでは、依存方向を明確にするためにBBTの領域で分ける。

各領域の内側では、その領域に合った分類軸を使う。

- Businessは、業務のFeatureで分ける
- Entry Pointは、入力経路と業務のFeatureで分ける
- Integrationは、業務のFeatureまたはBusinessが求める能力で分ける
- Infrastructureは、具体的なTechnologyで分ける
- Appsは、起動・配布する実行単位で分ける

```text
project/
├── apps/
│   ├── server/
│   ├── cli/
│   ├── worker/
│   └── migrate/
└── src/
    ├── business/
    │   ├── order/
    │   ├── customer/
    │   └── payment/
    ├── entrypoint/
    │   ├── http/
    │   │   ├── order/
    │   │   └── customer/
    │   ├── cli/
    │   └── worker/
    ├── integration/
    │   ├── order/
    │   ├── payment/
    │   └── notification/
    └── infrastructure/
        ├── mysql/
        ├── redis/
        ├── storage/
        ├── http/
        ├── cli/
        ├── messaging/
        ├── logging/
        ├── configuration/
        └── process/
```

トップレベルの分離で依存方向を守り、各領域の内側ではFeatureに関するコードを近くに置く。

物理的なディレクトリやモジュールの切り方は、言語とBuild Systemに合わせる。

## 3. 分離の粒度

BBTが分けるのは責務と依存であり、ファイル数やクラス数ではない。

- Operationは、クラス、関数、オブジェクト、モジュールのいずれでもよい
- 小さな変換は、HandlerやRequired Interfaceの実装の中に置ける
- 複雑な変換や共有する変換は、Mapperとして分ける
- Entry PointとIntegrationは、ORMやWeb FrameworkなどのAPIを直接利用できる
- Infrastructureには、共有するTechnologyの設定、型、実装を置く
- 抽象化は、共有する振る舞い、テストでの制御、複数実装の切り替え、変更影響の封じ込めなど、具体的な目的に合わせて導入する

小さなFeatureは少ないファイルで始め、名前を付ける価値のある責務が生まれたところを分ける。

## 4. Business

`business` は、アプリケーションが業務として行うことを表す。

主に次のコードを置く。

- 外部から実行できる業務操作
- 業務操作の入力、結果、エラー
- Entity、Value Object、Domain Model
- Policy、Domain Service
- Businessが外部へ求める能力を表すinterface
- Businessを構成するためのFactory

Businessの内側はPackage by Featureで整理する。

```text
business/
├── order/
│   ├── create-order-operation
│   ├── cancel-order-operation
│   ├── order-repository
│   ├── order
│   └── order-policy
├── customer/
└── payment/
```

### 4.1 公開面

Businessの公開面は、利用者ごとに分けて考える。

```text
Business
├── Inbound API
│   ├── Operations
│   ├── Operation Inputs
│   ├── Operation Results
│   └── Business Errors
├── Outbound API
│   ├── Required Interfaces
│   └── 実装に必要な型
├── Composition API
│   └── Factory / Module Builder
└── Internal
    ├── Domain Models
    ├── Policies
    ├── Domain Services
    └── Helpers
```

Entry PointはInbound APIを使う。

IntegrationはOutbound APIと、その実装に必要な型を使う。Repositoryの契約にDomain Modelが含まれる場合、その型はIntegrationから見える公開面になる。

AppsはComposition APIを使ってBusinessを組み立てる。

言語のModule、Package、Export、Build Target、Import Ruleなどを使い、利用者ごとに必要な公開面を作る。

### 4.2 Operation

Operationは、Businessの外部から実行できる一つの業務操作である。

標準の名前は `*Operation` とする。

```text
CreateOrderOperation
CancelOrderOperation
RegisterCustomerOperation
ExpireOrdersOperation
```

Operationは次の役割を持つ。

- 一連の業務処理を組み立てる
- Domain ModelやPolicyを呼び出す
- Businessが求めるinterfaceを利用する
- 業務上の分岐、成功、失敗を表す
- 整合性を保つ処理の範囲を決める

Entry PointはOperationを通してBusinessを呼び出す。

複数のDomain ModelやRepositoryを業務的な順序で扱う処理は、その流れを表すOperationとしてBusinessに置く。

## 5. Entry Point

`entrypoint` はInbound Boundaryである。

外部の入力をOperationの入力へ変換し、Operationの結果を外部の出力へ変換する。

```text
Technology Input
HTTP Request / CLI Input / Message Context
    ↓
Entry Point Handler
    ↓
Operation Input
    ↓
Business Operation
    ↓
Operation Result
    ↓
Technology Output
HTTP Response / Exit Result / Ack
```

Entry Pointは次の役割を持つ。

- Route、Command、Topic、JobとHandlerの対応を表す
- 入力を読み取り、構文や形式を検証する
- Operationの入力へ変換する
- Operationを呼び出す
- 結果やBusiness Errorを外部向けの出力へ変換する

文字列を数値として読めるか、必須項目が存在するかなど、入力形式の判断はEntry Pointが行う。

入力値が業務上正しいか、操作を実行できるかという判断はBusinessが行う。

Entry PointはHTTP Request、CLI Context、Message ContextなどのTechnology固有の型を使える。共通のRuntime ContextやHandler契約をプロジェクトで定義する場合は、Infrastructureに置く。

Web ServerやCLI Runtimeの起動、Loggerの生成、Dependency InjectionはAppsが行う。

## 6. Integration

`integration` はOutbound Boundaryである。

Businessが求める能力を、Database、Cache、Storage、外部APIなどを使って実現する。

```text
business/order/
└── OrderRepository

integration/order/
└── OrderRepositoryImpl
```

Integrationは次の役割を持つ。

- Businessが定義したRequired Interfaceを実装する
- Businessの型とTechnology上の型を変換する
- Required Interfaceの意味を満たすQueryやAPI呼び出しを組み立てる
- 複数のTechnologyを一つの実装として組み合わせる
- TechnologyのエラーをBusinessの契約に合う結果へ変換する
- Businessが求める整合性をTransaction、Outbox、Idempotencyなどで実現する
- Businessが許容する範囲でCache、Retry、Fallbackを構成する

一つの実装がMySQL、Redis、Storageなどを組み合わせてもよい。Integrationの単位は、利用するTechnologyではなく、実現するBusinessの能力である。

### 6.1 実装の名前

一つの標準実装だけがある場合は、次のような名前を使える。

```text
OrderRepositoryImpl
PaymentGatewayImpl
DocumentStorageImpl
```

実装の違いを名前で示す場合は、Businessから見た振る舞いや運用上の役割を表す。

```text
PersistentOrderRepository
CachedOrderRepository
ReadOnlyOrderRepository
PrimaryPaymentGateway
```

名前は、その実装がBusinessへ提供する役割を伝えるものにする。

### 6.2 変換

Businessの型とTechnology上の型の変換はIntegrationが担う。

変換が小さく一か所だけで使われる場合は、Required Interfaceの実装の中に置く。

変換が複雑な場合、複数の処理から使う場合、単独でテストする場合はMapperとして分ける。

```text
OrderRepositoryImpl
    ├── Query
    └── OrderPersistenceMapper
```

Mapperは、変換という責務を読みやすい大きさに保つために使う。

### 6.3 Technologyの利用

Integrationは、ORM、Database Driver、HTTP Clientなどのライブラリを直接使える。

プロジェクト共通のConnection、Schema、Record、Client設定、Transaction Primitiveなどがある場合はInfrastructureから利用する。

この分け方により、Businessの要求を実現するコードはIntegrationに集まり、Technologyに共通するコードはInfrastructureに集まる。

## 7. Infrastructure

`infrastructure` は、具体的なTechnologyと、そのTechnology上の表現を扱う。

Technologyを中心に整理する。

```text
infrastructure/
├── mysql/
├── redis/
├── storage/
├── http/
├── cli/
├── messaging/
├── logging/
├── configuration/
└── process/
```

Infrastructureは次のようなものを提供する。

- Database Connection、Driver、Transaction Primitive
- Table、Column、Index、Constraint、DDL
- Database Schema、Migration
- ORM Record、Row、Persistence Model
- Redis Record、Keyの技術的な構造
- Storage Object、Pathの技術的な構造
- 外部ServiceのClientとDTO
- HTTP Server、Router、Request Context、Response Context
- CLI Runtime、Message Runtime
- Logger、Configuration Loader、Process制御

Infrastructureには、`orders` テーブルや `OrderRecord` のようなアプリケーション固有の名前が現れる。これらはDatabaseやORM上の表現として扱う。

Infrastructureの依存先は、Infrastructure内のコードとTechnologyライブラリである。Businessの型、interface、Moduleは参照しない。

Businessの意味や判断はBusinessに置き、その意味をTechnologyへ接続する処理はBoundaryに置く。

Infrastructureの抽象化は、Technologyに関する共通動作や設定を集める目的で導入する。BoundaryからTechnologyライブラリを直接使う方が明確な場合は、その形を選ぶ。

## 8. Apps

`apps` は、BBTの各領域を実行可能なアプリケーションとして組み立てる。

実行単位ごとに分ける。

```text
apps/
├── server/
├── cli/
├── worker/
└── migrate/
```

Appsは次の役割を持つ。

- 利用する具体実装を選ぶ
- Objectを生成する
- Dependency Injectionを行う
- Route、Command、Consumer、Jobを登録する
- Configurationを各部品へ渡す
- 初期化の順序を決める
- Processを起動し、終了させる
- SignalやGraceful Shutdownを扱う

Appsのコードは、生成、選択、接続、登録、起動、終了の流れとして読めるようにする。

```text
config = loadConfig()
logger = createLogger(config)
database = createDatabase(config)
repository = createOrderRepository(database)
operations = createOrderOperations(repository)
handlers = createOrderHandlers(operations)
server = createHttpServer(logger)
registerRoutes(server, handlers)
run(server)
```

Composition Rootが大きくなる場合は、Appの内側をFeatureごとのComposition Moduleに分ける。

```text
apps/server/
├── main
├── composition/
│   ├── order
│   ├── customer
│   └── payment
└── config
```

各Composition Moduleは、各領域の公開されたFactoryや契約を使ってObject Graphの一部を作る。`main` はそれらをまとめて起動する。

Configuration LoaderやHTTP ServerそのものはInfrastructureが提供する。どの設定と実装を組み合わせるかはAppsが決める。

## 9. 整合性とTransaction

業務処理の整合性は、Businessの要求とTechnology