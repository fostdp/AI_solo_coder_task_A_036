# syntax=docker/dockerfile:1.6

FROM python:3.11-slim-bookworm

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    tzdata \
    && rm -rf /var/lib/apt/lists/*

# Set timezone
ENV TZ=Asia/Shanghai
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

# Create non-root user
RUN groupadd -r appgroup && useradd -r -g appgroup appuser

# Copy requirements and install Python dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy the simulator code
COPY sensor_simulator.py .

# Set proper ownership
RUN chown -R appuser:appgroup /app

# Switch to non-root user
USER appuser

# Default environment variables
ENV API_URL=http://backend:8000/api/v1/sensor
ENV TURBINE_COUNT=100
ENV STRAIN_SENSORS_PER_BLADE=20
ENV AE_SENSORS_PER_BLADE=10
ENV REPORT_INTERVAL=600
ENV DAMAGE_INJECTION_ENABLED=true
ENV DAMAGE_INJECTION_INTERVAL=300

# Health check
HEALTHCHECK --interval=60s --timeout=5s --start-period=30s --retries=3 \
    CMD python -c "import sys; import urllib.request; urllib.request.urlopen('http://backend:8000/health', timeout=5); print('OK')" || exit 1

# Run the simulator
ENTRYPOINT ["python", "sensor_simulator.py"]
