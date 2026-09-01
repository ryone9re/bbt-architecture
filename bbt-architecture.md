# BBT Architecture

**Business / Boundary / Technology Architecture**

- 文書種別: アーキテクチャ原則・設計ガイド
- バージョン: 1.1
- 基本領域: `business` / `entrypoint` / `integration` / `infrastructure`
- 実行可能コード: `apps`

---

## 1. 概要

BBT Architectureは、アプリケーションのコードを次の3つの関心に分離するための設計原則である。

- **Business**: 業務として何を行い、何を正しいと判断するか
- **Boundary**: Businessと外部世界の意味・形式・契約をどう接続するか
- **Technology**: HTTP、CLI、Database、Cache、Storageなどの具体技術をどう扱うか

Boundaryには方向があるため、再利用可能なコードは次の4領域に分ける。

```text
src/
├── business/
├── entrypoint/
├── integration/
└── infrastructure/
```

| 領域 | 関心 | 主な責務 |
|---|---|---|
| `business` | Business | 業務操作、業務ルール、外部へ要求する能力 |
| `entrypoint` | Inbound Boundary | 外部入力をBusiness Operationへ接続する |
| `integration` | Outbound Boundary | Businessの要求を具体技術によって実現する |
| `infrastructure` | Technology | 技術本体と、その技術上の表現を提供する |

これらを実行可能なアプリケーションとして組み立てるコードは、BBTの領域外にある `apps` に置く。

```text
project/
├── apps/
│   ├── server/
│   ├── cli/
│   └── worker/
└── src/
    ├── business/
    ├── entrypoint/
    ├── integration/
    └── infrastructure/
```

> **Businessは業務を担い、Technologyは技術を担い、Boundaryは両者を接続する。Appsはそれらを組み立てて実行する。**

> **Business does business. Technology does technology. Boundaries translate between them. Apps assemble and run them.**

---

## 2. 基本構造

```text
                         Business
                            ▲
                           / \
                          /   \
                         /     \
                Entry Point   Integration
                   Inbound      Outbound
                   Boundary     Boundary
                         \       /
                          \     /
                           ▼   ▼
                         Technology
                      Infrastructure
```

- `entrypoint` は外部世界からBusinessへ入るInbound Boundaryである。
- `integration` はBusinessが外部世界へ要求する能力を実現するOutbound Boundaryである。
- `infrastructure` は両Boundaryが利用する具体的なTechnologyを提供する。

### Apps

`apps` は5番目の層ではない。BBTによって分離された部品を特定の実行形態として構成する、実行可能コードの置き場である。

```text
apps/
├── server/
│   └── main
├── cli/
│   └── main
├── worker/
│   └── main
└── migrate/
    └── main
```

`apps` は依存グラフの根であり、他の領域から参照される部品ではない。

---

## 3. 依存ルール

```text
apps           -> business / entrypoint / integration / infrastructure
entrypoint     -> business / infrastructure
integration    -> business / infrastructure
business       -> BBTの他領域へ依存しない
infrastructure -> BBTの他領域へ依存しない
```

禁止する依存:

```text
business       -X-> entrypoint / integration / infrastructure / apps
entrypoint     -X-> integration / apps
integration    -X-> entrypoint / apps
infrastructure -X-> business / entrypoint / integration / apps
src/*          -X-> apps
```

| From \ To | Business | Entry Point | Integration | Infrastructure | Apps |
|---|---:|---:|---:|---:|---:|
| Business | 許可 | 禁止 | 禁止 | 禁止 | 禁止 |
| Entry Point | Inbound APIのみ | 許可 | 禁止 | Inbound技術契約のみ | 禁止 |
| Integration | Outbound APIのみ | 禁止 | 許可 | 許可 | 禁止 |
| Infrastructure | 禁止 | 禁止 | 禁止 | 許可 | 禁止 |
| Apps | Composition目的で許可 | 許可 | 許可 | 許可 | App内部のみ |

---

## 4. Business

`business` はアプリケーションが業務として実現する内容を表現する。

代表的には次を含む。

- Operation
- OperationのInput、Result、Business Error
- Domain Model
- Entity、Value Object
- Policy、Domain Service
- Businessが外部へ要求するinterface
- Businessを構成するための公開Factory

Business内部はコードの種類ではなく、業務上の関心事ごとに分割する。Package by Featureを基本とする。

```text
business/
├── order/
│   ├── create-order-operation
│   ├── cancel-order-operation
│   ├── order-repository
│   └── internal/
│       ├── order
│       └── order-policy
├── customer/
└── payment/
```

Businessの公開面は次のように制限する。

```text
Business
├── Inbound API
│   ├── Operations
│   ├── Operation Inputs
│   ├── Operation Results
│   └── Business Errors
├── Outbound API
│   ├── Required Interfaces
│   └── 実装に必要な最小限の契約
├── Composition API
│   └── OperationやFeatureを構成するFactory
└── Internal
    ├── Domain Models
    ├── Policies
    ├── Domain Services
    └── Internal Helpers
```

公開範囲の具体的な実現方法は言語のModule、Package、Export、Build Target、Import Ruleなどに委ねる。

- Entry PointはInbound APIのみを利用する。
- IntegrationはOutbound APIと実装に必要な最小限の契約のみを利用する。
- AppsはComposition APIを中心に利用し、Business内部を直接組み立てない。

---

## 5. Operation

OperationはBusinessの外部から実行可能な一つの業務操作である。標準命名は `*Operation` とする。

```text
CreateOrderOperation
CancelOrderOperation
RegisterCustomerOperation
ExpireOrdersOperation
```

Operationは、一連の業務処理のオーケストレーション、Domain ModelやPolicyの呼び出し、Required Interfaceの利用、業務上の分岐・成功・失敗・一貫性境界を表現する。

HTTP status、CLI exit code、Framework固有Request/Response、SQL、Redis、Storage、外部API、Server起動などは扱わない。

Entry Pointが複数のDomain Model、Policy、Repository、Operationを業務的な順序で組み合わせる必要が生じた場合、その組み合わせ自体を新しいOperationとしてBusinessへ定義する。

---

## 6. Entry Point

`entrypoint` はInbound Boundaryである。外部世界の入力をOperation Inputへ変換し、Operationの結果を外部世界の出力へ変換する。

```text
Infrastructure-owned Input Context
    ↓
Entry Point Handler
    ↓
Business Operation Input
    ↓
Business Operation
    ↓
Business Operation Result
    ↓
Infrastructure-owned Output Context
```

Entry Pointには、Route・Command・Topic・JobなどとHandlerの対応、入力形式の検証、Operationの呼び出し、結果やBusiness Errorから外部出力への変換を置く。

Domain Model、Policy、Repositoryの直接操作、業務フローの組み立て、SQLや外部APIの利用、Runtime起動、Dependency Injectionの構成は置かない。

Entry PointはBoundaryであるためBusinessとInbound Technologyの両方を知る。HTTP Request Context、CLI Input Context、Message Context、Handler契約などはInfrastructureが所有し、Entry Pointはそれらを利用する。

---

## 7. Integration

`integration` はOutbound Boundaryである。Businessが要求するinterfaceを、一つ以上のTechnologyを利用して実装する。

```text
business/order/
└── OrderRepository

integration/order/
└── OrderRepositoryImpl
```

Integrationには次を置く。

- Business Required Interfaceの実装
- Business ModelとTechnology Modelの相互変換・Mapper
- Required Interfaceの意味を満たすQueryやORM呼び出し
- Businessの処理単位とTechnology Transactionの対応
- Database、Cache、Storage、外部APIなどの組み合わせ
- Technology ErrorからBusiness契約上のErrorへの変換
- Business要求を実現するためのCache、Fallback、Retry、Consistency方針

実装は原則としてBusinessの要求を中心に命名する。

```text
OrderRepositoryImpl
PaymentGatewayImpl
DocumentStorageImpl
NotificationSenderImpl
```

単に内部で利用しているという理由だけで `MysqlOrderRepository` や `S3DocumentStorage` のように具体技術名を含めない。一つの実装が複数Technologyを組み合わせる可能性があるためである。

IntegrationはTechnology別ではなく、Business FeatureまたはRequired Interfaceを中心にPackage by Featureで整理する。

---

## 8. Infrastructure

`infrastructure` は具体的なTechnology本体と、そのTechnology上の表現を扱う。

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

> **InfrastructureはBusinessの型・interface・Moduleへ依存せず、Businessの意味や判断を実装しない。**

Business由来の単語を一切含まないことを要求するわけではない。Table名、Column名、外部API Field名など、Technology上の表現にはアプリケーション固有の名前が含まれ得る。

Infrastructureには、Connection、Transaction API、Driver、Table・Column・Index・Constraint、DDL、Database Schema、Migration、ORM Record、Redis Record、Storage Object、外部Service DTO、HTTP/CLI Runtime Context、Server、Router、Logger、Configuration Loaderなどを置く。

一方、Business Required Interfaceの実装、Domain ModelとのMapper、RepositoryとしてのQuery orchestration、業務上のCache/Retry/Fallback判断などはIntegrationへ置く。

---

## 9. Apps

`apps` はBBTの層ではない。BBTの各領域を特定の実行形態として組み立て、実行する。

```text
apps/
├── server/
├── cli/
├── worker/
└── migrate/
```

Appsには、具体実装の選択、Object生成、Dependency Injection、Route/Command/Consumer/Job登録、App固有Configuration適用、初期化順序、起動・終了、Signal処理、Graceful Shutdownなどを置く。

Business Logic、Boundary変換、Required Interface実装、Technology ModelやTechnology本体は置かない。

各AppのComposition Rootは、そのAppの `main` または同じApp配下のComposition Moduleに置く。

```text
apps/server/
├── main
├── composition
└── config
```

Appsが行うのは、**生成、選択、接続、登録、起動、終了**である。

Database SchemaとMigration定義はInfrastructureに置く。独立したMigration Executableが必要な場合は `apps/migrate` がInfrastructureのMigration RunnerとMigration定義を組み立てて実行する。

---

## 10. Persistenceの配置例

```text
src/
├── business/
│   └── order/
│       ├── create-order-operation
│       ├── order-repository
│       └── internal/
│           └── order
├── integration/
│   └── order/
│       ├── order-repository-impl
│       └── order-persistence-mapper
└── infrastructure/
    └── mysql/
        ├── connection
        ├── transaction
        ├── schema/
        │   └── orders-table
        ├── orm/
        │   └── order-record
        └── migrations/
```

保存時:

```text
CreateOrderOperation
    ↓
OrderRepository
    ↓
OrderRepositoryImpl
    ↓
OrderPersistenceMapper.toRecord(Order)
    ↓
OrdersTable / ORM Query API
    ↓
MySQL
```

読み込み時:

```text
MySQL
    ↓
OrdersTable / OrderRecord
    ↓
OrderPersistenceMapper.toBusiness(OrderRecord)
    ↓
OrderRepositoryImpl
    ↓
OrderRepository
    ↓
Business
```

| 要素 | 配置 | 理由 |
|---|---|---|
| `Order` | `business/order/internal` | Domain Model |
| `OrderRepository` | `business/order` | Businessが要求する能力 |
| `OrderRepositoryImpl` | `integration/order` | Required Interfaceの実現 |
| `OrderPersistenceMapper` | `integration/order` | Business ModelとDB Modelの変換 |
| Repository methodを満たすORM Query | `integration/order` | Business要求をDB操作として実現する |
| `OrdersTable` | `infrastructure/mysql/schema` | Database上のTable定義 |
| `OrderRecord` | `infrastructure/mysql/orm` | ORM上のPersistence Model |
| DDL、Index、Constraint | `infrastructure/mysql/schema` | Database上の技術表現 |
| Migration | `infrastructure/mysql/migrations` | Database Schemaの変更 |
| `MysqlConnection` | `infrastructure/mysql` | MySQL Technology本体 |
| Migrationの起動 | `apps/migrate` | ExecutableのCompositionと実行 |

SQLやORMを使っているという理由だけでは配置を決めない。

- Table単位の生成Query APIや汎用CRUD機構 → `infrastructure`
- `OrderRepository.findById` などRequired Interfaceの意味を満たすQuery → `integration`
- 「この状態のOrderは取得対象にできるか」という業務判断 → `business`

---

## 11. 推奨ディレクトリ構成

```text
project/
├── apps/
│   ├── server/
│   │   ├── main
│   │   ├── composition
│   │   └── config
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

この構成は概念モデルであり、物理ディレクトリや可視性の実現方法は言語とBuild Systemに合わせて調整してよい。

---

## 12. 配置判断

コードをどこに置くかは、「どこから呼ばれるか」や「どのLibraryを使うか」ではなく、**何を責務とするか**で判断する。

```text
何を業務として行うか？
    -> Business

外部