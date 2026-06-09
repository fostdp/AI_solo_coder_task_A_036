import "influxdata/influxdb/v1"

option v = {bucket: "wind_turbine_blades", org: "wind_farm", token: "your-token-here"}

bucket(name: "wind_turbine_blades", retentionPeriod: 31536000000000)

v1.dbrpMappingCreate(
    org: "wind_farm",
    bucketID: "wind_turbine_blades",
    database: "wind_turbine_blades",
    retentionPolicy: "autogen",
    default: true
)

task option task = {
    name: "strain_hourly_agg",
    every: 1h
}

data = from(bucket: "wind_turbine_blades")
    |> range(start: -task.every)
    |> filter(fn: (r) => r._measurement == "strain_data")
    |> aggregateWindow(
        every: 1h,
        fn: (tables=<-, column) => tables |> mean(column: "_value"),
        createEmpty: false
    )
    |> to(bucket: "wind_turbine_blades", org: "wind_farm")

task option task = {
    name: "ae_hourly_agg",
    every: 1h
}

from(bucket: "wind_turbine_blades")
    |> range(start: -task.every)
    |> filter(fn: (r) => r._measurement == "ae_events")
    |> count()
    |> set(key: "_field", value: "event_count")
    |> to(bucket: "wind_turbine_blades", org: "wind_farm")
