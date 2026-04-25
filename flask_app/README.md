
## Stock symbol examples
```
MSFT
BHP.AX
```

## Run Locally
```
python -m venv venv

source venv/bin/activate

pip install -r requirements.txt

python3 app.py
```

## Build and Run Docker
```
docker build -t stock-tracker-flask .

docker run --rm -p 5000:5000 -v stock_data:/app/data --name stock-tracker-flask stock-tracker-flask

docker run -d -p 5000:5000 -v stock_data:/app/data --name stock-tracker-flask stock-tracker-flask

docker volume inspect stock_data
sudo ls -la /var/lib/docker/volumes/stock_data/_data
```

## Pull from GitHub
```
docker pull ghcr.io/faush01/stock-tracker-flask:main

docker run --rm -p 5000:5000 -v stock_data:/app/data --name stock-tracker ghcr.io/faush01/stock-tracker-flask:main

docker run -d -p 5000:5000 -v stock_data:/app/data --name stock-tracker ghcr.io/faush01/stock-tracker-flask:main
```