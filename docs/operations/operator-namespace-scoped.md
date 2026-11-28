# Namespace-scoped operator mode

This page tells a cluster admin how to run the Sift operator in
namespace-scoped mode. In this mode, the operator does not create role
bindings. The admin creates some objects by hand. For every setting, see
[configuration.md](configuration.md).

## What this mode changes

Cluster-wide mode is the default. In that mode, the operator holds a wide
`ClusterRole` and creates the auth-delegator `ClusterRoleBinding` for each
Sift.

In namespace-scoped mode:

- The operator uses the bundle `k8s/operator/namespace-scoped/`. Its
  `ClusterRole` `sift-operator` has no write on `clusterrolebindings` and no
  `bind` rule. It has one read rule: `get` on `clusterrolebindings`, to
  report the condition.
- The operator creates workload objects only in the namespaces that the
  admin lists. Each listed namespace gets one `Role` and one `RoleBinding`.
- The admin creates the auth-delegator `ClusterRoleBinding` for each Sift
  that uses `auth: kubernetes`.
- The operator reports the `AuthDelegatorBinding` condition.

This page describes the manifests and the code. It states no measured result.

## Install

1. Apply the CRD and the reduced cluster role:

   ```sh
   kubectl apply -k k8s/operator/namespace-scoped
   ```

   This bundle replaces `k8s/operator/cluster/`. It holds the CRD (`../../crd`),
   the `ClusterRole` `sift-operator` and its `ClusterRoleBinding`.

2. Optional: create the `sift-system` namespace:

   ```sh
   kubectl apply -k k8s/operator/namespace
   ```

3. Set the watched namespaces, then install the operator. Copy the directory
   `k8s/operator/namespace-scoped/watch-namespaces`. In the copy, edit
   `watch-namespaces-patch.yaml` to list your namespaces. Then apply the copy.
   It includes `k8s/operator/namespaced` and sets `WATCH_NAMESPACES`. Apply it
   in place of `k8s/operator/namespaced`:

   ```sh
   kubectl apply -k <copy-of-watch-namespaces>
   ```

4. Turn on the mode. Set the environment variable
   `SIFT_OPERATOR_PRECREATED_AUTH_DELEGATOR` on the operator Deployment (see
   [the variable](#the-authdelegatorbinding-condition)):

   ```sh
   kubectl -n sift-system set env deployment/sift-operator SIFT_OPERATOR_PRECREATED_AUTH_DELEGATOR=true
   ```

5. For each watched namespace, apply one copy of
   `k8s/operator/namespaced-rbac/example`. Copy the directory, set
   `namespace:` in the copy to the watched namespace, and apply the copy:

   ```sh
   kubectl apply -k <copy-of-namespaced-rbac-example>
   ```

   The `Role` `sift-operator` lets the operator create these objects in that
   namespace only: Services, StatefulSets, Deployments, DaemonSets,
   ConfigMaps, ServiceAccounts, PersistentVolumeClaims, NetworkPolicies,
   CronJobs, PodDisruptionBudgets, `roles` and `rolebindings`. The
   `RoleBinding` subject is the ServiceAccount `sift-operator` in
   `sift-system`.

## Objects the cluster admin creates

For each Sift with `auth: kubernetes`, the admin creates one
`ClusterRoleBinding`:

- name: `sift.<namespace>.<name>.auth-delegator`, where `<namespace>` and
  `<name>` are the namespace and the name of the Sift;
- `roleRef`: the ClusterRole `system:auth-delegator`;
- label: `sift.axiom.dev/owner-namespace: <namespace>`. The operator sets
  this label, so the admin copies it;
- subjects: ServiceAccounts in the namespace of the Sift. A single-node
  Sift has `<name>`. An HA Sift has `<name>`, `<name>-store` and
  `<name>-control`.

This sample is for an HA Sift named `prod` in the namespace `team-a`:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: sift.team-a.prod.auth-delegator
  labels:
    sift.axiom.dev/owner-namespace: team-a
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: system:auth-delegator
subjects:
  - kind: ServiceAccount
    name: prod
    namespace: team-a
  - kind: ServiceAccount
    name: prod-store
    namespace: team-a
  - kind: ServiceAccount
    name: prod-control
    namespace: team-a
```

Apply the file:

```sh
kubectl apply -f <file>
```

The metrics agent needs a `Role` and a `RoleBinding` named
`<name>-agent-project` in the namespace of an HA Sift. The operator creates
them. The admin does not write them. The admin only applies the
`namespaced-rbac` copy for that namespace, so the operator may create `roles`
and `rolebindings` there. The `Role` of `namespaced-rbac` holds `get` and
`list` on `pods`. Kubernetes lets the operator delegate only the rights that
the operator holds itself.

## The AuthDelegatorBinding condition

The operator sets the condition `AuthDelegatorBinding` only when:

- `SIFT_OPERATOR_PRECREATED_AUTH_DELEGATOR` is `true`, and
- the Sift has `auth: kubernetes`.

The condition has these values:

- `False` with reason `PrecreatedBindingMissing`: the ClusterRoleBinding does
  not exist. The message says that the cluster admin creates it before the
  Sift is ready.
- `True` with reason `PrecreatedBindingFound`: the ClusterRoleBinding exists.

To read the condition:

```sh
kubectl -n <namespace> get sift <name> -o jsonpath='{.status.conditions[?(@.type=="AuthDelegatorBinding")]}'
```

Only the exact value `true` turns the mode on. With that value, the operator
does not render, create or prune the auth-delegator ClusterRoleBinding. Any
other value keeps the default, where the operator creates it.

## Watched namespaces

Set `WATCH_NAMESPACES` on the operator Deployment. It is a comma list, for
example `team-a,team-b`. Blank parts are ignored. An empty value watches all
namespaces. The base `k8s/operator/namespaced/deployment.yaml` sets an empty
value.

List each namespace that gets a `namespaced-rbac` copy. Otherwise the
operator cannot create workload objects in that namespace.

## Default mode

Cluster-wide mode stays the default. To use it, apply `k8s/operator/cluster/`
and `k8s/operator/namespaced`. It needs none of the steps on this page.
